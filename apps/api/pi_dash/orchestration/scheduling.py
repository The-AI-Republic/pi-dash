# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Periodic agent ticking — primitives for the per-issue ticker clock.

This module owns the per-issue ``IssueAgentTicker`` row: when to arm it,
when to disarm it, when to reset it, how to dispatch a continuation run on
a tick, and how to apply the deferred cap-hit pause.

This is internal continuation-cadence machinery, system-armed on Issue
state transitions; it is not a user-authored periodic-job system.

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
from pi_dash.db.models.issue_agent_ticker import (
    DEFAULT_INTERVAL_SECONDS,
    DEFAULT_MAX_TICKS,
    INFINITE_MAX_TICKS,
    IssueAgentTicker,
    TickerDisarmReason,
    jitter_seconds,
)
from pi_dash.orchestration.agent_phases import is_ticking_state
from pi_dash.runner.models import AgentRun

logger = logging.getLogger(__name__)


PAUSED_STATE_NAME = "Paused"

# Retained for backwards compatibility with callers / tests that import
# the literal. New code should use
# ``orchestration.agent_phases.is_ticking_state`` /
# ``phase_config_for`` instead. The literal still names the In Progress
# phase's state — see ``agent_phases.PHASES``.
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


def arm_ticker(
    issue: Issue,
    *,
    dispatch_immediate: bool = True,  # noqa: ARG001 — caller-only signal
) -> IssueAgentTicker:
    """Create or reset the ticker for an issue entering a ticking state.

    ``dispatch_immediate`` does not affect the schedule itself — arming
    *never* fires a run. The flag exists only to document the caller's
    intent: ``True`` means the caller is firing (or expects another
    immediate dispatch), ``False`` means arming alone is enough — no
    new run should be inferred from this call. Used by Comment & Run
    on a Paused issue (§4.6) and by re-arm-on-comment in
    :func:`pi_dash.orchestration.service.handle_issue_comment`.

    Honors ``user_disabled`` and project-level ``agent_ticking_enabled``:
    sets ``enabled = False`` when either suppresses ticks.
    """
    suppress_for_project = not _project_ticking_enabled(issue)

    with transaction.atomic():
        sched = (
            IssueAgentTicker.objects.select_for_update()
            .filter(issue=issue)
            .first()
        )
        if sched is None:
            # Brand-new row — single INSERT with the right values. No
            # follow-up UPDATE needed.
            interval = _project_default_interval(issue)
            sched = IssueAgentTicker.objects.create(
                issue=issue,
                interval_seconds=None,
                max_ticks=None,
                user_disabled=False,
                next_run_at=_compute_next_run_at(interval),
                tick_count=0,
                enabled=not suppress_for_project,
                disarm_reason=TickerDisarmReason.NONE,
            )
        else:
            # Existing row — reset clock and re-evaluate enabled.
            sched.tick_count = 0
            sched.next_run_at = _compute_next_run_at(
                sched.effective_interval_seconds()
            )
            suppress = sched.user_disabled or suppress_for_project
            sched.enabled = not suppress
            # Re-arming clears any prior disarm cause; if we end up
            # disabled because of user_disabled or project-suppress,
            # the next disarm caller will set the appropriate reason.
            sched.disarm_reason = TickerDisarmReason.NONE
            sched.save(
                update_fields=[
                    "tick_count",
                    "next_run_at",
                    "enabled",
                    "disarm_reason",
                    "updated_at",
                ]
            )

    logger.info(
        "agent_ticker: armed issue=%s enabled=%s next_run_at=%s",
        issue.pk,
        sched.enabled,
        sched.next_run_at,
    )
    return sched


def disarm_ticker(
    issue: Issue,
    *,
    reason: str = TickerDisarmReason.LEFT_TICKING_STATE,
) -> Optional[IssueAgentTicker]:
    """Set ``enabled = False`` and persist ``disarm_reason``. Idempotent.

    Called when issue leaves a ticking group, on cap hit, on terminal
    done-signal, and when the user toggles ``user_disabled = True``
    mid-flight. Returns the schedule if one exists; ``None`` otherwise.

    The ``reason`` argument is **load-bearing**: only ``CAP_HIT``
    triggers ``maybe_apply_deferred_pause`` to auto-Pause the issue.
    Terminal-signal disarms (``completed`` / ``blocked``) leave the
    issue in place for the human to act.

    Note: this helper *overwrites* ``disarm_reason`` even when the
    ticker is already disabled — callers are expected to know which
    cause should win. This differs from
    :func:`maybe_disarm_on_terminal_signal`, which deliberately
    preserves the prior reason so an opportunistic terminal-signal
    disarm cannot clobber a pre-existing ``CAP_HIT``. Today the only
    production caller is the state-transition handler with the
    default ``LEFT_TICKING_STATE`` reason; future callers passing
    ``TERMINAL_SIGNAL`` should prefer
    :func:`maybe_disarm_on_terminal_signal` instead.
    """
    with transaction.atomic():
        sched = (
            IssueAgentTicker.objects.select_for_update()
            .filter(issue=issue)
            .first()
        )
        if sched is None:
            return None
        # Always update the reason — even when already disabled — so a
        # later disarm with a different reason (e.g., terminal signal
        # arriving on an already cap-hit-disabled ticker) does not
        # incorrectly preserve the older reason. But the rule is that
        # the *first* terminal cause wins for the auto-pause gate, so
        # only the first transition flips ``enabled``.
        changed = False
        if sched.enabled:
            sched.enabled = False
            changed = True
        if sched.disarm_reason != reason:
            sched.disarm_reason = reason
            changed = True
        if changed:
            sched.save(
                update_fields=["enabled", "disarm_reason", "updated_at"]
            )
    logger.info(
        "agent_ticker: disarmed issue=%s reason=%s", issue.pk, reason
    )
    return sched


def maybe_disarm_on_terminal_signal(run: AgentRun) -> bool:
    """Disarm the ticker if the run's done-payload status is a phase-
    final signal (``completed`` or ``blocked``).

    Do not disarm on ``noop`` (which currently persists as
    ``AgentRunStatus.COMPLETED``). The hook inspects
    ``run.done_payload['status']`` rather than ``run.status`` because
    ``noop`` and true completion share the same persisted run status.

    **Critical:** only disarm when the ticker is currently *armed*. If
    the ticker is already disabled, leave the existing
    ``disarm_reason`` alone — that prior reason (typically ``CAP_HIT``
    when cap was hit during the same tick that produced this run)
    determines whether ``maybe_apply_deferred_pause`` will auto-Pause.
    Overwriting a ``CAP_HIT`` reason with ``TERMINAL_SIGNAL`` would
    silently drop the auto-pause.

    Closes the gap in the existing issue-ticking design: the spec
    says terminal completed/blocked should disarm but the existing
    code only disarms via state transitions. Idempotent. Safe to
    call alongside ``maybe_apply_deferred_pause``.

    Returns ``True`` when a disarm was applied this call (i.e., the
    ticker was armed and is now disabled with reason
    ``TERMINAL_SIGNAL``).
    """
    if run.work_item_id is None:
        return False
    payload = run.done_payload or {}
    payload_status = (payload or {}).get("status")
    if payload_status not in {"completed", "blocked"}:
        return False
    issue = run.work_item
    with transaction.atomic():
        sched = (
            IssueAgentTicker.objects.select_for_update()
            .filter(issue=issue)
            .first()
        )
        if sched is None:
            return False
        # Already disabled — preserve the prior reason. See docstring.
        if not sched.enabled:
            return False
        sched.enabled = False
        sched.disarm_reason = TickerDisarmReason.TERMINAL_SIGNAL
        sched.save(
            update_fields=["enabled", "disarm_reason", "updated_at"]
        )
    logger.info(
        "agent_ticker: disarmed-on-terminal issue=%s payload_status=%s",
        issue.pk,
        payload_status,
    )
    return True


def reset_ticker_after_comment_and_run(issue: Issue) -> Optional[IssueAgentTicker]:
    """Reset ``tick_count = 0`` and ``next_run_at = NOW() + interval + jitter``.

    Called by the Comment & Run handler after the run is dispatched
    (§4.6 step 4). Uses ``select_for_update`` to serialize against
    ``fire_tick`` (§6.1).
    """
    with transaction.atomic():
        sched = (
            IssueAgentTicker.objects.select_for_update()
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
        sched.disarm_reason = TickerDisarmReason.NONE
        sched.save(
            update_fields=[
                "tick_count",
                "next_run_at",
                "enabled",
                "disarm_reason",
                "updated_at",
            ]
        )
    logger.info(
        "agent_ticker: reset issue=%s next_run_at=%s",
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
    # Pods are project-scoped — fall back to the issue's project default.
    # Issues without a project (shouldn't exist post-refactor) return None
    # and the caller surfaces an error rather than silently routing into
    # a workspace-wide pod.
    if issue.project_id is None:
        return None
    return Pod.default_for_project_id(issue.project_id)


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
            "agent_ticker: skip dispatch issue=%s reason=active-run-exists triggered_by=%s",
            issue.pk,
            triggered_by,
        )
        return None

    parent = orchestration_service._latest_prior_run(issue)
    if parent is None:
        logger.info(
            "agent_ticker: skip dispatch issue=%s reason=no-prior-run triggered_by=%s",
            issue.pk,
            triggered_by,
        )
        return None

    creator = _resolve_creator_for_trigger(
        issue, triggered_by=triggered_by, actor=actor
    )
    if creator is None:
        logger.warning(
            "agent_ticker: skip dispatch issue=%s reason=no-creator triggered_by=%s",
            issue.pk,
            triggered_by,
        )
        return None

    pod = _resolve_pod_for_issue(issue)
    if pod is None:
        logger.warning(
            "agent_ticker: skip dispatch issue=%s reason=no-pod triggered_by=%s",
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
    """If the schedule was disarmed by **cap exhaustion**, the issue is
    still in a ticking state, and no other active runs exist on the
    issue, transition the issue → Paused.

    Idempotent — only the first concurrent terminate event takes effect.
    Returns ``True`` when a transition was applied, ``False`` otherwise.

    Gated on ``disarm_reason == CAP_HIT``: terminal-signal disarms
    (``completed``/``blocked``) leave the issue in place for the human
    to act. ``LEFT_TICKING_STATE`` and ``USER_DISABLED`` likewise are
    not auto-pause causes.

    Called from the runner Channels consumer after persisting a terminal
    run status.
    """
    if run.work_item_id is None:
        return False

    issue = run.work_item
    sched = IssueAgentTicker.objects.filter(issue=issue).first()
    if sched is None or sched.enabled:
        return False
    if sched.disarm_reason != TickerDisarmReason.CAP_HIT:
        return False

    state = issue.state
    if state is None:
        return False
    if not is_ticking_state(state):
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
            "agent_ticker: cannot auto-pause issue=%s — no Paused state in project",
            issue.pk,
        )
        return False

    from pi_dash.orchestration.workpad import get_agent_system_user

    bot = get_agent_system_user()
    with transaction.atomic():
        # Re-fetch the schedule under a row lock so the disarmed-check we
        # made above stays valid for the rest of this transaction. Without
        # this, a concurrent ``arm_ticker`` (e.g. user manually re-starts
        # the issue between the unlocked read on line ~314 and here) can
        # re-enable the schedule while we auto-pause its issue.
        locked_sched = (
            IssueAgentTicker.objects.select_for_update()
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
        "agent_ticker: auto-paused issue=%s after cap hit",
        issue.pk,
    )
    return True


__all__ = [
    "DEFAULT_INTERVAL_SECONDS",
    "DEFAULT_MAX_TICKS",
    "DELEGATION_STATE_NAME",
    "INFINITE_MAX_TICKS",
    "PAUSED_STATE_NAME",
    "TRIGGER_COMMENT_AND_RUN",
    "TRIGGER_TICK",
    "arm_ticker",
    "disarm_ticker",
    "dispatch_continuation_run",
    "maybe_apply_deferred_pause",
    "maybe_disarm_on_terminal_signal",
    "reset_ticker_after_comment_and_run",
]
