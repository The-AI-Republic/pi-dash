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

from pi_dash.db.models.issue import Issue
from pi_dash.db.models.state import State, StateGroup
from pi_dash.prompting.composer import build_first_turn
from pi_dash.prompting.renderer import PromptRenderError
from pi_dash.runner.models import AgentRun, AgentRunStatus

logger = logging.getLogger(__name__)

#: The default state name that triggers delegation. Workspaces with bespoke
#: state names will need a separate policy pass before this broadens.
DELEGATION_STATE_NAME = "In Progress"


@dataclass
class TransitionOutcome:
    """What `handle_issue_state_transition` decided to do."""

    created_run: Optional[AgentRun] = None
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
) -> TransitionOutcome:
    """React to an issue state change.

    Creates an ``AgentRun`` (and dispatches it to the runner) when the
    transition matches the MVP trigger rule (``Todo -> In Progress``) and the
    single-active-run guardrail allows it.
    """
    if not _is_delegation_trigger(to_state):
        return TransitionOutcome(reason="not-a-trigger-state")

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
    return Pod.default_for_workspace_id(issue.workspace_id)


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
