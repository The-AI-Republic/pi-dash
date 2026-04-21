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

    owner = actor or _resolve_owner(issue)
    if owner is None:
        logger.warning(
            "orchestration: no owner for issue %s; skipping run creation",
            issue.id,
        )
        return TransitionOutcome(reason="no-owner")

    return _create_and_dispatch_run(issue=issue, parent=parent, owner=owner)


def _resolve_owner(issue: Issue):
    """Pick the user the run is billed to. Prefer the issue's creator; fall back
    to the project lead. The runner matcher uses this to pick a runner owned by
    the same user."""
    if issue.created_by_id:
        return issue.created_by
    project = issue.project
    return project.project_lead or project.default_assignee


def _create_and_dispatch_run(
    *, issue: Issue, parent: Optional[AgentRun], owner
) -> TransitionOutcome:
    with transaction.atomic():
        run = AgentRun.objects.create(
            owner=owner,
            workspace=issue.workspace,
            work_item=issue,
            parent_run=parent,
            status=AgentRunStatus.QUEUED,
            prompt="",  # populated below before dispatch
            run_config={
                "repo_url": (issue.project.repo_url or None),
                "repo_ref": (issue.project.base_branch or None),
                "git_work_branch": (issue.git_work_branch or None),
            },
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
        transaction.on_commit(lambda: _dispatch_to_runner(run.id))

    return TransitionOutcome(created_run=run, reason="created")


def _dispatch_to_runner(run_id) -> None:
    """Match a runner and forward the Assign envelope.

    Called after commit so the daemon never sees a run that hasn't landed yet.
    """
    from pi_dash.runner.services import matcher
    from pi_dash.runner.services.pubsub import send_to_runner

    with transaction.atomic():
        try:
            run = AgentRun.objects.select_for_update().get(id=run_id)
        except AgentRun.DoesNotExist:
            logger.warning("orchestration: run %s disappeared before dispatch", run_id)
            return

        if run.status != AgentRunStatus.QUEUED:
            return  # someone else already handled it

        chosen = matcher.select_runner_for_run(run)
        if chosen is None:
            logger.info("orchestration: no runner available for run %s; leaving queued", run.id)
            return

        run.runner = chosen
        run.status = AgentRunStatus.ASSIGNED
        run.assigned_at = timezone.now()
        run.save(update_fields=["runner", "status", "assigned_at"])
        chosen_id = chosen.id
        assign_msg = {
            "v": 1,
            "type": "assign",
            "run_id": str(run.id),
            "work_item_id": str(run.work_item_id) if run.work_item_id else None,
            "prompt": run.prompt,
            "repo_url": run.run_config.get("repo_url"),
            "repo_ref": run.run_config.get("repo_ref"),
            "git_work_branch": run.run_config.get("git_work_branch"),
            "expected_codex_model": run.run_config.get("model"),
            "approval_policy_overrides": run.run_config.get(
                "approval_policy_overrides"
            ),
            "deadline": None,
        }

    send_to_runner(chosen_id, assign_msg)
