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
from pi_dash.orchestration.agent_phases import is_ticking_state
from pi_dash.prompting.composer import build_continuation, build_first_turn
from pi_dash.prompting.renderer import PromptRenderError
from pi_dash.runner.models import AgentRun, AgentRunStatus, Pod, Runner, RunnerStatus

logger = logging.getLogger(__name__)

#: Retained for backwards compatibility with imports. New code should use
#: ``orchestration.agent_phases.is_ticking_state`` /
#: ``phase_config_for`` instead.
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
    """A state transition triggers a run when ``to_state`` is one of the
    registered ticking states (see ``orchestration.agent_phases.PHASES``).

    Custom workspace state names within a ticking group still do not
    trigger — the registry pins the literal state name per group.
    """
    return is_ticking_state(to_state)


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
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker
    from pi_dash.orchestration import scheduling

    # Detect cross-phase transition (both states are ticking but in
    # different groups). On started → review, capture the latest impl
    # run as the resume parent so the reverse transition can restore
    # that exact session. We capture **before** the disarm/re-arm
    # below so the captured run is genuinely the latest pre-review.
    cross_phase = (
        is_ticking_state(from_state)
        and is_ticking_state(to_state)
        and from_state.group != to_state.group
    )
    if cross_phase and from_state.group == StateGroup.STARTED.value:
        latest_impl = _latest_prior_run(issue)
        if latest_impl is not None:
            IssueAgentTicker.objects.filter(issue=issue).update(
                resume_parent_run=latest_impl
            )

    # Disarm when leaving a ticking state into anything that isn't the
    # *same* ticking state. Inter-phase transitions (e.g., In Progress
    # → In Review) intentionally disarm-then-re-arm so the ticker row
    # lands on the new phase's effective cadence.
    if is_ticking_state(from_state) and (
        not is_ticking_state(to_state) or from_state.group != to_state.group
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

    # Parent resolution for the dispatched run depends on the phase
    # transition shape:
    #
    # - Cross-phase entry into a phase whose ``fresh_session_on_entry``
    #   is True (today: review): dispatch with parent_run=None and
    #   pinned_runner_id cleared. The new phase's prompt template lands
    #   as the actual system prompt rather than as a user-turn message
    #   on a resumed session.
    # - Cross-phase entry into a phase whose ``fresh_session_on_entry``
    #   is False (today: started, e.g., review → In Progress
    #   hand-back): use ``ticker.resume_parent_run`` (the impl run we
    #   stashed on the forward transition) as the parent so the agent
    #   resumes the original implementation thread instead of
    #   parenting off a review run.
    # - Same-phase or first-time entry: today's behavior — parent =
    #   _latest_prior_run(issue).
    fresh_session = False
    parent = _latest_prior_run(issue)
    to_cfg = phase_config_for(to_state)
    if cross_phase and to_cfg is not None:
        if to_cfg.fresh_session_on_entry:
            fresh_session = True
            parent = None
        else:
            ticker = IssueAgentTicker.objects.filter(issue=issue).first()
            if ticker is not None and ticker.resume_parent_run_id is not None:
                parent = ticker.resume_parent_run
            else:
                # No resume target captured. This happens when the
                # forward path skipped the impl phase (e.g.,
                # Todo → In Review → In Progress): there's no
                # implementation session to resume, and the latest
                # prior run is a review run — wrong parent for an
                # impl-phase dispatch. Fall back to a fresh session
                # rather than parenting off the review run.
                fresh_session = True
                parent = None

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
        issue=issue,
        parent=parent,
        creator=creator,
        pod=pod,
        fresh_session=fresh_session,
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
#: Derived from the phase registry — every group that has a registered
#: ticking phase. Backlog/Unstarted are owned by the state-transition
#: trigger; Completed/Cancelled require the user to re-open the issue.
#: See ``.ai_design/create_review_state/design.md`` §7.4.
CONTINUATION_ELIGIBLE_GROUPS = tuple(PHASES.keys())


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

    # Re-arm on human comment. Comment is engagement; engagement
    # restarts automatic ticking (see design §4.6). Honors
    # ``user_disabled`` via ``arm_ticker``. Done before any of the
    # coalesce / active-run / no-pod early returns so the ticker
    # restarts even when this specific comment doesn't dispatch a
    # new run (the existing run / queued follow-up will pick up the
    # comment, and the next tick happens automatically).
    from pi_dash.orchestration import scheduling

    if is_ticking_state(issue.state):
        try:
            scheduling.arm_ticker(issue, dispatch_immediate=False)
        except Exception:
            logger.exception(
                "orchestration.continuation: re-arm failed for issue=%s",
                issue.pk,
            )

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
    *,
    issue: Issue,
    parent: Optional[AgentRun],
    creator,
    pod,
    fresh_session: bool = False,
) -> TransitionOutcome:
    """Create a fresh ``AgentRun`` and dispatch it.

    ``fresh_session=True`` clears ``parent_run`` and ``pinned_runner``
    so the new phase's template body lands as the system prompt of a
    brand-new agent session (rather than as a user-turn message on a
    resumed prior session). Used by cross-phase entry into a phase
    whose ``fresh_session_on_entry`` is True.
    """
    from pi_dash.runner.services import matcher

    with transaction.atomic():
        effective_parent = None if fresh_session else parent
        # Pin the new run to the parent run's runner when eligible.
        # Skip pinning for fresh sessions — they intentionally drop
        # session continuity.
        pinned_runner = None
        if not fresh_session and effective_parent is not None:
            pinned_runner = _pinned_runner_for(effective_parent)
        run = AgentRun.objects.create(
            workspace=issue.workspace,
            created_by=creator,
            pod=pod,
            work_item=issue,
            parent_run=effective_parent,
            pinned_runner=pinned_runner,
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
