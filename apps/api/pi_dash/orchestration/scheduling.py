# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Periodic agent ticking — scheduling primitives.

This module owns the per-issue ``IssueAgentSchedule`` row: when to arm it,
when to disarm it, when to reset it, how to dispatch a continuation run on
a tick, and how to apply the deferred cap-hit pause.

See ``.ai_design/issue_ticking_system/design.md`` for the full design;
quick links:

- §4 lifecycle (arm / disarm / re-arm)
- §4.4.1 deferred cap-hit pause
- §4.6 Comment & Run reset
- §6 scanner + atomic claim
- §12.2 public API surface
"""

from __future__ import annotations

import logging
from datetime import timedelta
from typing import Optional

from django.db import transaction
from django.utils import timezone

from pi_dash.db.models.issue import Issue
from pi_dash.db.models.issue_agent_schedule import (
    DEFAULT_INTERVAL_SECONDS,
    DEFAULT_MAX_TICKS,
    INFINITE_MAX_TICKS,
    IssueAgentSchedule,
    jitter_seconds,
)
from pi_dash.db.models.state import StateGroup
from pi_dash.runner.models import AgentRun

logger = logging.getLogger(__name__)


PAUSED_STATE_NAME = "Paused"
DELEGATION_STATE_NAME = "In Progress"


# ---------------------------------------------------------------------------
# Arming / disarming
# ---------------------------------------------------------------------------


def _project_default_interval(issue: Issue) -> int:
    project = issue.project
    return getattr(project, "agent_default_interval_seconds", DEFAULT_INTERVAL_SECONDS)


def _project_default_max_ticks(issue: Issue) -> int:
    project = issue.project
    return getattr(project, "agent_default_max_ticks", DEFAULT_MAX_TICKS)


def _project_ticking_enabled(issue: Issue) -> bool:
    project = issue.project
    return bool(getattr(project, "agent_ticking_enabled", True))


def _compute_next_run_at(interval_seconds: int, *, base=None):
    base = base or timezone.now()
    return base + timedelta(seconds=interval_seconds + jitter_seconds(interval_seconds))


def arm_schedule(
    issue: Issue,
    *,
    dispatch_immediate: bool = True,  # noqa: ARG001 — caller-only signal
) -> IssueAgentSchedule:
    """Create or reset the schedule for an issue entering Started/In Progress.

    ``dispatch_immediate`` does not affect the schedule itself — arming
    *never* fires a run. The flag exists only to document the caller's
    intent: ``True`` means the state-transition handler is firing the
    immediate dispatch on its own; ``False`` means another path
    (Comment & Run on a Paused issue, §4.6) owns the dispatch and arming
    should not duplicate it.

    Honors ``user_disabled`` and project-level ``agent_ticking_enabled``:
    sets ``enabled = False`` when either suppresses ticks.
    """
    suppress_for_project = not _project_ticking_enabled(issue)

    with transaction.atomic():
        sched = (
            IssueAgentSchedule.objects.select_for_update()
            .filter(issue=issue)
            .first()
        )
        if sched is None:
            # Brand-new row — single INSERT with the right values. No
            # follow-up UPDATE needed.
            interval = _project_default_interval(issue)
            sched = IssueAgentSchedule.objects.create(
                issue=issue,
                interval_seconds=None,
                max_ticks=None,
                user_disabled=False,
                next_run_at=_compute_next_run_at(interval),
                tick_count=0,
                enabled=not suppress_for_project,
            )
        else:
            # Existing row — reset clock and re-evaluate enabled.
            sched.tick_count = 0
            sched.next_run_at = _compute_next_run_at(
                sched.effective_interval_seconds()
            )
            suppress = sched.user_disabled or suppress_for_project
            sched.enabled = not suppress
            sched.save(
                update_fields=[
                    "tick_count",
                    "next_run_at",
                    "enabled",
                    "updated_at",
                ]
            )

    logger.info(
        "agent_schedule: armed issue=%s enabled=%s next_run_at=%s",
        issue.pk,
        sched.enabled,
        sched.next_run_at,
    )
    return sched


def disarm_schedule(issue: Issue) -> Optional[IssueAgentSchedule]:
    """Set ``enabled = False``. Idempotent.

    Called when issue leaves Started, on cap hit, on terminal done-signal,
    and when the user toggles ``user_disabled = True`` mid-flight.
    Returns the schedule if one exists; ``None`` otherwise.
    """
    with transaction.atomic():
        sched = (
            IssueAgentSchedule.objects.select_for_update()
            .filter(issue=issue)
            .first()
        )
        if sched is None:
            return None
        if sched.enabled:
            sched.enabled = False
            sched.save(update_fields=["enabled", "updated_at"])
    logger.info("agent_schedule: disarmed issue=%s", issue.pk)
    return sched


def reset_schedule_after_comment_and_run(issue: Issue) -> Optional[IssueAgentSchedule]:
    """Reset ``tick_count = 0`` and ``next_run_at = NOW() + interval + jitter``.

    Called by the Comment & Run handler after the run is dispatched
    (§4.6 step 4). Uses ``select_for_update`` to serialize against
    ``fire_tick`` (§6.1).
    """
    with transaction.atomic():
        sched = (
            IssueAgentSchedule.objects.select_for_update()
            .filter(issue=issue)
            .first()
        )
        if sched is None:
            return None
        sched.tick_count = 0
        sched.next_run_at = _compute_next_run_at(sched.effective_interval_seconds())
        # Comment & Run is an explicit re-engagement — re-enable unless
        # the user has explicitly disabled ticking on this issue.
        suppress = sched.user_disabled or not _project_ticking_enabled(issue)
        sched.enabled = not suppress
        sched.save(
            update_fields=[
                "tick_count",
                "next_run_at",
                "enabled",
                "updated_at",
            ]
        )
    logger.info(
        "agent_schedule: reset issue=%s next_run_at=%s",
        issue.pk,
        sched.next_run_at,
    )
    return sched


# ---------------------------------------------------------------------------
# Continuation dispatch
# ---------------------------------------------------------------------------


TRIGGER_TICK = "tick"
TRIGGER_COMMENT_AND_RUN = "comment_and_run"


def _resolve_pod_for_issue(issue: Issue):
    from pi_dash.runner.models import Pod

    if issue.assigned_pod_id is not None:
        pinned = Pod.objects.filter(pk=issue.assigned_pod_id).first()
        if pinned is not None:
            return pinned
    return Pod.default_for_workspace_id(issue.workspace_id)


def _resolve_creator_for_trigger(issue: Issue, *, triggered_by: str, actor=None):
    """Pick the user who will appear as ``AgentRun.created_by``."""
    if actor is not None:
        return actor
    if triggered_by == TRIGGER_TICK:
        # System-driven; attribute to the agent system bot.
        from pi_dash.orchestration.workpad import get_agent_system_user
        return get_agent_system_user()
    # Fall back to the issue creator / project lead — same logic the
    # state-transition path uses.
    if issue.created_by_id:
        return issue.created_by
    project = issue.project
    return project.project_lead or project.default_assignee


def dispatch_continuation_run(
    issue: Issue,
    *,
    triggered_by: str,
    actor=None,
) -> Optional[AgentRun]:
    """Public wrapper for tick / Comment & Run dispatch.

    Resolves parent (latest prior run), creator (system bot for ticks;
    explicit ``actor`` for Comment & Run), pod, then delegates to
    :func:`pi_dash.orchestration.service._create_continuation_run`.
    Returns the created run, or ``None`` when the single-active-run
    guardrail blocks creation or no pod is available.
    """
    from pi_dash.orchestration import service as orchestration_service

    if orchestration_service._active_run_for(issue) is not None:
        logger.info(
            "agent_schedule: skip dispatch issue=%s reason=active-run-exists triggered_by=%s",
            issue.pk,
            triggered_by,
        )
        return None

    parent = orchestration_service._latest_prior_run(issue)
    if parent is None:
        logger.info(
            "agent_schedule: skip dispatch issue=%s reason=no-prior-run triggered_by=%s",
            issue.pk,
            triggered_by,
        )
        return None

    creator = _resolve_creator_for_trigger(
        issue, triggered_by=triggered_by, actor=actor
    )
    if creator is None:
        logger.warning(
            "agent_schedule: skip dispatch issue=%s reason=no-creator triggered_by=%s",
            issue.pk,
            triggered_by,
        )
        return None

    pod = _resolve_pod_for_issue(issue)
    if pod is None:
        logger.warning(
            "agent_schedule: skip dispatch issue=%s reason=no-pod triggered_by=%s",
            issue.pk,
            triggered_by,
        )
        return None

    outcome = orchestration_service._create_continuation_run(
        issue=issue,
        parent=parent,
        creator=creator,
        pod=pod,
    )
    return outcome.created_run


# ---------------------------------------------------------------------------
# Deferred cap-hit pause (§4.4.1)
# ---------------------------------------------------------------------------


def maybe_apply_deferred_pause(run: AgentRun) -> bool:
    """If the schedule is disarmed, the issue is still in Started, and no
    other active runs exist on the issue, transition the issue
    In Progress → Paused.

    Idempotent — only the first concurrent terminate event takes effect.
    Returns ``True`` when a transition was applied, ``False`` otherwise.

    Called from the runner Channels consumer after persisting a terminal
    run status.
    """
    if run.work_item_id is None:
        return False

    issue = run.work_item
    sched = IssueAgentSchedule.objects.filter(issue=issue).first()
    if sched is None or sched.enabled:
        return False

    state = issue.state
    if state is None:
        return False
    if state.group != StateGroup.STARTED.value:
        return False
    if state.name != DELEGATION_STATE_NAME:
        return False

    # Other active runs (besides the one that just terminated) keep the
    # issue alive in In Progress — the next terminate will check again.
    from pi_dash.orchestration import service as orchestration_service

    active = orchestration_service._active_run_for(issue)
    if active is not None and active.pk != run.pk:
        return False

    paused_state = (
        type(state).all_state_objects.filter(
            project=issue.project,
            name=PAUSED_STATE_NAME,
            deleted_at__isnull=True,
        )
        .first()
    )
    if paused_state is None:
        logger.warning(
            "agent_schedule: cannot auto-pause issue=%s — no Paused state in project",
            issue.pk,
        )
        return False

    from pi_dash.orchestration.workpad import get_agent_system_user

    bot = get_agent_system_user()
    with transaction.atomic():
        # Re-fetch the schedule under a row lock so the disarmed-check we
        # made above stays valid for the rest of this transaction. Without
        # this, a concurrent ``arm_schedule`` (e.g. user manually re-starts
        # the issue between the unlocked read on line ~314 and here) can
        # re-enable the schedule while we auto-pause its issue.
        locked_sched = (
            IssueAgentSchedule.objects.select_for_update()
            .filter(pk=sched.pk)
            .first()
        )
        if locked_sched is None or locked_sched.enabled:
            return False
        # Re-fetch the issue under the same transaction to guard against a
        # racing state transition.
        IssueModel = type(issue)
        locked = (
            IssueModel.all_objects.select_for_update().filter(pk=issue.pk).first()
        )
        if locked is None:
            return False
        if locked.state_id != state.id:
            return False
        locked.state = paused_state
        if hasattr(locked, "updated_by"):
            locked.updated_by = bot
        locked.save(update_fields=["state", "updated_at"])

    logger.info(
        "agent_schedule: auto-paused issue=%s after cap hit",
        issue.pk,
    )
    return True


__all__ = [
    "DEFAULT_INTERVAL_SECONDS",
    "DEFAULT_MAX_TICKS",
    "INFINITE_MAX_TICKS",
    "PAUSED_STATE_NAME",
    "TRIGGER_COMMENT_AND_RUN",
    "TRIGGER_TICK",
    "arm_schedule",
    "disarm_schedule",
    "dispatch_continuation_run",
    "maybe_apply_deferred_pause",
    "reset_schedule_after_comment_and_run",
]
