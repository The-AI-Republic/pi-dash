# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Orchestration service.

This module owns the decision of *whether* a state transition creates an
``AgentRun``, enforces the single-active-run guardrail, renders the prompt, and
hands the run off to the runner dispatcher. Views and signals call in here —
they must not duplicate this logic.

See `.ai_design/prompt_system/prompt-system-design.md` §6 and §8.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Optional

from django.db import transaction
from django.utils import timezone

from pi_dash.db.models.issue import Issue, IssueComment
from pi_dash.db.models.state import State, StateGroup
from pi_dash.prompting.composer import build_continuation, build_first_turn
from pi_dash.prompting.renderer import PromptRenderError
from pi_dash.runner.models import AgentRun, AgentRunStatus, Pod, Runner, RunnerStatus

logger = logging.getLogger(__name__)

#: The default state name that triggers delegation. Workspaces with bespoke
#: state names will need a separate policy pass before this broadens.
DELEGATION_STATE_NAME = "In Progress"


@dataclass
class TransitionOutcome:
    """What `handle_issue_state_transition` decided to do."""

    created_run: Optional[AgentRun] = None
    reason: str = ""


@dataclass
class ContinuationOutcome:
    """What ``handle_issue_comment`` decided to do."""

    created_run: Optional[AgentRun] = None
    coalesced_into: Optional[AgentRun] = None
    reason: str = ""


def _active_run_for(issue: Issue) -> Optional[AgentRun]:
    return (
        AgentRun.objects.filter(work_item=issue)
        .filter(
            status__in=[
                AgentRunStatus.QUEUED,
                AgentRunStatus.ASSIGNED,
                AgentRunStatus.RUNNING,
                AgentRunStatus.AWAITING_APPROVAL,
                AgentRunStatus.AWAITING_REAUTH,
            ]
        )
        .order_by("-created_at")
        .first()
    )


def _latest_prior_run(issue: Issue) -> Optional[AgentRun]:
    return (
        AgentRun.objects.filter(work_item=issue).order_by("-created_at").first()
    )


def _is_delegation_trigger(to_state: Optional[State]) -> bool:
    """MVP: only the default-named "In Progress" state in the ``started`` group
    triggers a run. Custom started-group states are deferred.
    """
    if to_state is None:
        return False
    if to_state.group != StateGroup.STARTED.value:
        return False
    return to_state.name == DELEGATION_STATE_NAME


def handle_issue_state_transition(
    issue: Issue,
    from_state: Optional[State],
    to_state: Optional[State],
    actor=None,
    *,
    dispatch_immediate: bool = True,
) -> TransitionOutcome:
    """React to an issue state change.

    Creates an ``AgentRun`` (and dispatches it to the runner) when the
    transition matches the MVP trigger rule (``Todo -> In Progress``) and the
    single-active-run guardrail allows it.

    Also arms/disarms the per-issue ticker:

    - Entering the literal "In Progress" state arms the ticker (or
      re-arms it on Paused → In Progress).
    - Leaving the Started group disarms the ticker.

    ``dispatch_immediate=False`` lets a caller (e.g., the Comment & Run
    flow re-opening a Paused issue) arm the ticker without firing the
    state-transition's own immediate dispatch — the caller will dispatch
    its own run.
    """
    from pi_dash.orchestration import scheduling

    from_group = from_state.group if from_state is not None else None
    to_group = to_state.group if to_state is not None else None

    # Disarm when leaving the Started group, regardless of where we land.
    if (
        from_group == StateGroup.STARTED.value
        and to_group != StateGroup.STARTED.value
    ):
        scheduling.disarm_ticker(issue)

    if not _is_delegation_trigger(to_state):
        return TransitionOutcome(reason="not-a-trigger-state")

    # Arming is independent of whether the immediate dispatch is fired by
    # this handler or by the caller — the ticker is the steady-state tick
    # source either way.
    scheduling.arm_ticker(issue, dispatch_immediate=dispatch_immediate)

    if not dispatch_immediate:
        return TransitionOutcome(reason="dispatch-deferred-to-caller")

    existing_active = _active_run_for(issue)
    if existing_active is not None:
        logger.info(
            "orchestration: skip run creation for issue %s — active run %s",
            issue.id,
            existing_active.id,
        )
        return TransitionOutcome(reason="active-run-exists")

    parent = _latest_prior_run(issue)

    creator = actor or _resolve_fallback_creator(issue)
    if creator is None:
        logger.warning(
            "orchestration: no creator for issue %s; skipping run creation",
            issue.id,
        )
        return TransitionOutcome(reason="no-creator")

    pod = _resolve_pod_for_issue(issue)
    if pod is None:
        logger.warning(
            "orchestration: no pod available for workspace %s; "
            "leaving issue %s unhandled",
            issue.workspace_id,
            issue.id,
        )
        return TransitionOutcome(reason="no-pod-available")

    return _create_and_dispatch_run(
        issue=issue, parent=parent, creator=creator, pod=pod
    )


def _resolve_fallback_creator(issue: Issue):
    """Pick a fallback ``created_by`` for legacy callers that don't pass ``actor``.

    Renamed from ``_resolve_owner``: under the new model the returned user
    becomes ``AgentRun.created_by`` (the access principal), not ``owner``
    (the billable party).

    Prefer the issue's creator; fall back to the project lead. New call sites
    must always pass ``actor`` explicitly; this helper is a back-compat shim.
    """
    if issue.created_by_id:
        return issue.created_by
    project = issue.project
    return project.project_lead or project.default_assignee


def _resolve_pod_for_issue(issue: Issue):
    """Resolve the pod a run for this issue should belong to.

    Priority: ``issue.assigned_pod`` if set and active, else
    ``workspace.default_pod``. Returns ``None`` only when both are gone (a
    pathological state since invariant #13 keeps a default pod alive).
    """
    from pi_dash.runner.models import Pod

    if issue.assigned_pod_id is not None:
        pinned = Pod.objects.filter(pk=issue.assigned_pod_id).first()
        if pinned is not None:
            return pinned
    # Pods are project-scoped — fall back to the issue's project default.
    # An issue without a project (shouldn't exist post-refactor) returns
    # None; callers surface the error rather than dispatch into a
    # workspace-wide pod.
    if issue.project_id is None:
        return None
    return Pod.default_for_project_id(issue.project_id)


#: Issue state groups in which a comment is allowed to wake the agent.
#: Backlog/Unstarted are owned by the state-transition trigger; Completed/
#: Cancelled require the user to re-open the issue. See §5.2 of
#: .ai_design/issue_run_improve/design.md.
CONTINUATION_ELIGIBLE_GROUPS = (StateGroup.STARTED.value,)


def handle_issue_comment(comment: IssueComment) -> ContinuationOutcome:
    """React to a comment on an issue. Maybe wake the agent.

    The flow:
    1. Skip if the comment was authored by a bot (the agent's own workpad
       updates would otherwise trigger continuation in a loop).
    2. Skip when the issue's state group disallows continuation (see
       :data:`CONTINUATION_ELIGIBLE_GROUPS`).
    3. Coalesce: if a QUEUED follow-up already exists for the issue,
       leave it alone — the prompt builder will pick up the new comment
       at dispatch time via :func:`build_continuation`.
    4. Skip if the latest run is itself ``is_active`` (RUNNING / ASSIGNED
       / AWAITING_*). The terminate-side sweep will pick the comment up
       when that run finishes.
    5. Otherwise create R_next, pin it to the prior runner when eligible,
       and trigger drain.
    """
    if comment.actor_id is None:
        logger.info("orchestration.continuation: skip comment=%s reason=no-actor", comment.pk)
        return ContinuationOutcome(reason="no-actor")
    if comment.actor.is_bot:
        logger.info("orchestration.continuation: skip comment=%s reason=bot-comment", comment.pk)
        return ContinuationOutcome(reason="bot-comment")

    issue = comment.issue
    state_group = issue.state.group if issue.state else None
    if state_group not in CONTINUATION_ELIGIBLE_GROUPS:
        logger.info(
            "orchestration.continuation: skip issue=%s reason=state-not-eligible group=%s",
            issue.pk, state_group,
        )
        return ContinuationOutcome(reason="state-not-eligible")

    prior = _latest_prior_run(issue)
    if prior is None:
        logger.info("orchestration.continuation: skip issue=%s reason=no-prior-run", issue.pk)
        return ContinuationOutcome(reason="no-prior-run")

    # Coalesce against an already-queued follow-up. The prompt builder
    # rebuilds the continuation prompt from comments at dispatch time.
    queued_follow_up = (
        AgentRun.objects.filter(
            work_item=issue, status=AgentRunStatus.QUEUED
        )
        .order_by("-created_at")
        .first()
    )
    if queued_follow_up is not None:
        return ContinuationOutcome(
            coalesced_into=queued_follow_up, reason="coalesced"
        )

    # Don't wake while a run is already in flight; terminate sweep handles it.
    if prior.is_active:
        return ContinuationOutcome(reason="prior-run-active")

    pod = _resolve_pod_for_issue(issue)
    if pod is None:
        logger.warning(
            "orchestration.continuation: no pod for workspace %s; "
            "leaving issue %s unhandled",
            issue.workspace_id,
            issue.id,
        )
        return ContinuationOutcome(reason="no-pod-available")

    return _create_continuation_run(
        issue=issue,
        parent=prior,
        creator=comment.actor,
        pod=pod,
    )


def _create_continuation_run(
    *, issue: Issue, parent: AgentRun, creator, pod
) -> ContinuationOutcome:
    """Create R_next as a continuation of ``parent`` with optional pin."""
    from pi_dash.runner.services import matcher

    pinned_runner = _pinned_runner_for(parent)

    with transaction.atomic():
        run = AgentRun.objects.create(
            workspace=issue.workspace,
            created_by=creator,
            pod=pod,
            work_item=issue,
            parent_run=parent,
            pinned_runner=pinned_runner,
            status=AgentRunStatus.QUEUED,
            prompt="",
            run_config={
                "repo_url": (issue.project.repo_url or None),
                "repo_ref": (issue.project.base_branch or None),
                "git_work_branch": (issue.git_work_branch or None),
            },
        )
        try:
            run.prompt = build_continuation(issue, run)
        except PromptRenderError as exc:
            run.status = AgentRunStatus.FAILED
            run.error = f"prompt render failed: {exc}"
            run.ended_at = timezone.now()
            run.save(update_fields=["status", "error", "ended_at"])
            logger.exception(
                "orchestration.continuation: prompt render failed for issue %s",
                issue.id,
            )
            return ContinuationOutcome(created_run=run, reason="render-failed")
        run.save(update_fields=["prompt"])
        transaction.on_commit(lambda: matcher.drain_pod_by_id(pod.id))

    return ContinuationOutcome(created_run=run, reason="created")


def _pinned_runner_for(parent: AgentRun) -> Optional[Runner]:
    """Return the runner to pin a follow-up to, or None.

    Pin only when the parent has a session id we can resume against and
    its runner is still online-eligible. Otherwise leave the new run
    unpinned so any runner in the pod can take it (with a fresh-context
    fallback).
    """
    if not parent.thread_id or parent.runner_id is None:
        return None
    runner = parent.runner
    if runner is None:
        return None
    if runner.status == RunnerStatus.REVOKED:
        return None
    return runner


def _create_and_dispatch_run(
    *, issue: Issue, parent: Optional[AgentRun], creator, pod
) -> TransitionOutcome:
    from pi_dash.runner.services import matcher

    with transaction.atomic():
        run = AgentRun.objects.create(
            workspace=issue.workspace,
            created_by=creator,
            pod=pod,
            work_item=issue,
            parent_run=parent,
            status=AgentRunStatus.QUEUED,
            prompt="",  # populated below before dispatch
            run_config={
                "repo_url": (issue.project.repo_url or None),
                "repo_ref": (issue.project.base_branch or None),
                "git_work_branch": (issue.git_work_branch or None),
            },
            # owner stays NULL until assignment captures runner.owner.
        )
        try:
            run.prompt = build_first_turn(issue, run)
        except PromptRenderError as exc:
            run.status = AgentRunStatus.FAILED
            run.error = f"prompt render failed: {exc}"
            run.ended_at = timezone.now()
            run.save(update_fields=["status", "error", "ended_at"])
            logger.exception("orchestration: prompt render failed for issue %s", issue.id)
            return TransitionOutcome(created_run=run, reason="render-failed")

        run.save(update_fields=["prompt"])
        # Drain the pod's queue after the run row has landed. drain_pod is
        # idempotent and uses select_for_update(skip_locked=True), so it's
        # safe to run unconditionally.
        transaction.on_commit(lambda: matcher.drain_pod_by_id(pod.id))

    return TransitionOutcome(created_run=run, reason="created")


# ----------------------------------------------------------------------
# Project-scoped scheduler dispatch
#
# Schedulers are project-bound, not issue-bound. The existing
# ``_create_continuation_run`` and ``_create_and_dispatch_run`` paths both
# require an Issue (as ``work_item``), a project repo, and an issue-derived
# pod — none of which apply to a scheduler tick. This is the parallel path
# for ``SchedulerBinding``-driven runs.
#
# See ``.ai_design/project_scheduler/design.md`` §6.3.
# ----------------------------------------------------------------------


def dispatch_scheduler_run(
    binding, prompt: str
) -> tuple[Optional[AgentRun], Optional[str]]:
    """Create a fresh ``AgentRun`` for one scheduler-binding tick.

    The run is project-scoped: ``work_item=None``, ``parent_run=None``,
    ``scheduler_binding=binding``. Repo / workspace selection is the
    runner's problem (the dispatcher does not populate ``run_config``
    with repo URL / ref).

    Returns ``(run, None)`` on success, or ``(None, reason)`` when the
    dispatch was short-circuited. The reason string is what the Beat
    fire path stores on ``binding.last_error`` so operators don't need
    to grep worker logs to find out why a tick was skipped.
    """
    from pi_dash.runner.services import matcher

    pod = Pod.default_for_workspace_id(binding.workspace_id)
    if pod is None:
        logger.warning(
            "scheduler.dispatch: skip binding=%s reason=no-default-pod workspace=%s",
            binding.pk,
            binding.workspace_id,
        )
        return None, f"no default pod for workspace {binding.workspace_id}"

    creator = binding.actor
    if creator is None:
        from pi_dash.orchestration.workpad import get_agent_system_user
        creator = get_agent_system_user()
    if creator is None:
        logger.warning(
            "scheduler.dispatch: skip binding=%s reason=no-creator", binding.pk
        )
        return None, "no creator (binding.actor and system bot both unavailable)"

    with transaction.atomic():
        run = AgentRun.objects.create(
            workspace_id=binding.workspace_id,
            created_by=creator,
            pod=pod,
            work_item=None,
            scheduler_binding=binding,
            parent_run=None,
            status=AgentRunStatus.QUEUED,
            prompt=prompt or "",
            run_config={},
        )
        transaction.on_commit(lambda: matcher.drain_pod_by_id(pod.id))

    logger.info(
        "scheduler.dispatch: created run=%s binding=%s pod=%s",
        run.pk,
        binding.pk,
        pod.pk,
    )
    return run, None
