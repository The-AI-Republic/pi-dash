# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Periodic agent ticking — one clock per issue.

This module owns the per-issue ``IssueAgentTicker`` row. Every event that
can change the clock (the issue entering / moving within / leaving the
ticking bucket, a run ending with an outcome, a human asking for a run, a
Re-tick) goes through :func:`reconcile`; it also dispatches continuation
runs on a tick and applies the deferred cap-hit pause.

This is internal continuation-cadence machinery, system-armed on Issue
state transitions; it is not a user-authored periodic-job system.

See ``.ai_design/ticking_relevance/design.md`` (§4.0 event table, §4.5
entry-run queue, §5 budget, §7 outcomes, §10.1 ``reconcile``) and, for the
scanner / atomic claim, ``.ai_design/issue_ticking_system/design.md`` §6.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from datetime import timedelta
from typing import Any, Optional

from django.db import transaction
from django.utils import timezone

from pi_dash.db.models.issue import Issue
from pi_dash.db.models.issue_agent_ticker import (
    DEFAULT_INTERVAL_SECONDS,
    DEFAULT_MAX_TICKS,
    DEFAULT_RETICK_GRANT,
    INFINITE_MAX_TICKS,
    IssueAgentTicker,
    TickerDisarmReason,
    jitter_seconds,
)
from pi_dash.orchestration.agent_phases import is_ticking_state
from pi_dash.runner.models import AgentRun, AgentRunStatus, AgentRunTrigger

logger = logging.getLogger(__name__)


PAUSED_STATE_NAME = "Paused"

# DEPRECATED: retained only for backward compatibility with external
# importers (tests, integrations). Internal callers must use
# ``orchestration.agent_phases.is_ticking_state`` /
# ``phase_config_for`` — the literal still names the In Progress
# phase's state, see ``agent_phases.PHASES``. Remove once no
# remaining imports of ``DELEGATION_STATE_NAME`` exist.
DELEGATION_STATE_NAME = "In Progress"



# ---------------------------------------------------------------------------
# One clock per issue — ``reconcile``
# ---------------------------------------------------------------------------
#
# Every event that can change the ticker goes through ``reconcile``. The
# older entry points (``arm_ticker``, ``disarm_ticker``,
# ``reset_ticker_after_comment_and_run``, ``re_tick_ticker``,
# ``maybe_disarm_on_terminal_signal``) are thin senders of one event each
# and are kept so callers and tests migrate gradually.
#
# The event table in ``.ai_design/ticking_relevance/design.md`` §4.0 is the
# specification; ``fire_tick`` (``bgtasks.agent_ticker``) is the only writer
# of ``used``.


#: Outcomes a run may report through ``pidash run yield`` (design §7).
OUTCOME_PROGRESSED = "progressed"
OUTCOME_WAITING_ON_HUMAN = "waiting_on_human"
OUTCOME_WAITING_ON_EXTERNAL = "waiting_on_external"
OUTCOME_DONE = "done"
OUTCOME_BLOCKED = "blocked"
RUN_OUTCOMES = frozenset(
    {
        OUTCOME_PROGRESSED,
        OUTCOME_WAITING_ON_HUMAN,
        OUTCOME_WAITING_ON_EXTERNAL,
        OUTCOME_DONE,
        OUTCOME_BLOCKED,
    }
)
#: Outcomes that stop the clock for the stage the run was rendered for.
STOPPING_OUTCOMES = frozenset({OUTCOME_DONE, OUTCOME_BLOCKED, OUTCOME_WAITING_ON_HUMAN})
#: Legacy done-payload statuses from the Cloud Agent structured result and
#: the pre-yield fence, mapped onto the §7 vocabulary. ``noop`` is per kind
#: (see :func:`normalize_outcome`): "nothing changed" keeps an In Progress
#: clock ticking and stops a review / test one.
_LEGACY_OUTCOME_ALIASES = {
    "completed": OUTCOME_DONE,
    "paused": OUTCOME_WAITING_ON_HUMAN,
}


class TickerEventKind:
    ENTERED_BUCKET = "entered_bucket"
    MOVED_STAGE = "moved_stage"
    LEFT_BUCKET = "left_bucket"
    RUN_ENDED = "run_ended"
    HUMAN_RUN_REQUESTED = "human_run_requested"
    RETICK = "retick"


@dataclass(frozen=True)
class TickerEvent:
    """One thing that happened to an issue that the clock must react to."""

    kind: str
    #: For ``ENTERED_BUCKET`` / ``MOVED_STAGE``: the run that made the move,
    #: when an agent made it from inside a run (design §5.6). ``None`` means
    #: a human made the move.
    moved_by_run: Optional[AgentRun] = None
    #: For ``ENTERED_BUCKET`` / ``MOVED_STAGE``: the latest implementation
    #: run to remember for a later hand-back, when leaving In Progress.
    resume_parent: Optional[AgentRun] = None
    #: For ``RUN_ENDED``: the run and its reported outcome (``None`` when the
    #: run exited without yielding — a per-kind default applies).
    run: Optional[AgentRun] = None
    outcome: Optional[str] = None
    #: For ``HUMAN_RUN_REQUESTED`` / ``RETICK``: whether the caller intends to
    #: dispatch a run right now (so the clock only needs re-timing) — or
    #: wants ``reconcile`` to queue one if the issue is busy.
    want_run: bool = True
    #: For human levers: who asked, and the ``AgentRunTrigger`` the run
    #: should carry. Remembered on the ticker when the entry has to be
    #: queued, so the run that fires later is created as that person
    #: (LLM config, runner eligibility) and labelled correctly.
    actor: Optional[Any] = None
    trigger: str = ""

    # Convenience constructors — keep call sites readable.
    @classmethod
    def entered_bucket(cls, *, moved_by_run=None, resume_parent=None, want_run=True, actor=None):
        return cls(
            TickerEventKind.ENTERED_BUCKET,
            moved_by_run=moved_by_run,
            resume_parent=resume_parent,
            want_run=want_run,
            actor=actor,
            trigger=AgentRunTrigger.STATE_TRANSITION.value,
        )

    @classmethod
    def moved_stage(cls, *, moved_by_run=None, resume_parent=None, want_run=True, actor=None):
        return cls(
            TickerEventKind.MOVED_STAGE,
            moved_by_run=moved_by_run,
            resume_parent=resume_parent,
            want_run=want_run,
            actor=actor,
            trigger=AgentRunTrigger.STATE_TRANSITION.value,
        )

    @classmethod
    def left_bucket(cls):
        return cls(TickerEventKind.LEFT_BUCKET)

    @classmethod
    def run_ended(cls, run: AgentRun, outcome: Optional[str] = None):
        return cls(TickerEventKind.RUN_ENDED, run=run, outcome=outcome)

    @classmethod
    def human_run_requested(cls, *, want_run: bool = True, actor=None, trigger: str = ""):
        return cls(
            TickerEventKind.HUMAN_RUN_REQUESTED,
            want_run=want_run,
            actor=actor,
            trigger=trigger or AgentRunTrigger.RUN_AI.value,
        )

    @classmethod
    def retick(cls, *, want_run: bool = True, actor=None):
        return cls(
            TickerEventKind.RETICK,
            want_run=want_run,
            actor=actor,
            trigger=AgentRunTrigger.RUN_AI.value,
        )


@dataclass
class TickerDecision:
    """What ``reconcile`` decided, for the caller to act on."""

    ticker: Optional[IssueAgentTicker] = None
    #: The caller should create and dispatch a run *now* (the issue is free
    #: and a human asked for one). ``reconcile`` never creates runs itself.
    dispatch_now: bool = False
    #: An entry run was queued on the clock (design §4.5) — the issue was
    #: busy, or an agent made the move. ``fire_tick`` will pick it up.
    queued: bool = False
    #: An agent moved the issue while the pool is spent: nothing fires; the
    #: issue parks in its truthful state (design §5.4).
    parked: bool = False
    #: A Re-tick that granted budget.
    granted: bool = False
    reason: str = ""


def is_paused_state(state) -> bool:
    """The project's auto-pause parking state (``PAUSED_STATE_NAME``)."""
    return state is not None and getattr(state, "name", None) == PAUSED_STATE_NAME


def _project_ticking_enabled(issue: Issue) -> bool:
    project = getattr(issue, "project", None)
    return bool(getattr(project, "agent_ticking_enabled", True))


def _clock_allowed(issue: Issue, ticker: IssueAgentTicker) -> bool:
    """May this issue's clock run at all (user / project switches)?"""
    return not ticker.user_disabled and _project_ticking_enabled(issue)


def _compute_next_run_at(interval_seconds: int, *, base=None):
    base = base or timezone.now()
    return base + timedelta(seconds=interval_seconds + jitter_seconds(interval_seconds))


def _issue_has_active_run(issue: Issue) -> bool:
    from pi_dash.orchestration import service as orchestration_service

    return orchestration_service._active_run_for(issue) is not None


def _lock_ticker(issue: Issue, *, create: bool) -> Optional[IssueAgentTicker]:
    """Row-lock the issue's ticker, creating it (disabled, unarmed) if asked.

    Callers hold ``transaction.atomic()``.
    """
    ticker = IssueAgentTicker.objects.select_for_update().filter(issue=issue).first()
    if ticker is None and create:
        ticker = IssueAgentTicker.objects.create(
            issue=issue,
            user_disabled=False,
            next_run_at=None,
            used=0,
            granted=0,
            enabled=False,
            disarm_reason=TickerDisarmReason.NONE,
        )
    if ticker is not None:
        # Bind the caller's (fresh) issue so the phase-aware resolvers read
        # the state the caller sees rather than a lazily-loaded stale copy.
        ticker.issue = issue
    return ticker


_TICKER_CLOCK_FIELDS = (
    "enabled",
    "disarm_reason",
    "next_run_at",
    "pending_entry",
    "pending_entry_free",
    "pending_entry_actor",
    "pending_entry_trigger",
    "granted",
    "resume_parent_run",
    "updated_at",
)


def _save_clock(ticker: IssueAgentTicker) -> None:
    ticker.save(update_fields=list(_TICKER_CLOCK_FIELDS))


def _clear_pending(ticker: IssueAgentTicker) -> None:
    ticker.pending_entry = False
    ticker.pending_entry_free = False
    ticker.pending_entry_actor = None
    ticker.pending_entry_trigger = ""


def _stop_clock(ticker: IssueAgentTicker, reason: str) -> None:
    ticker.enabled = False
    ticker.disarm_reason = reason
    _clear_pending(ticker)


def _queue_entry(ticker: IssueAgentTicker, *, free: bool, actor=None, trigger: str = "") -> None:
    """Owe an entry run for the current stage (design §4.5).

    ``next_run_at = now`` so the scanner picks it up on its next pass;
    ``fire_tick`` refuses while a run is active and leaves the clock
    untouched, so the entry fires as soon as the issue is free. ``enabled``
    must be ``True`` even when the pool is spent and the entry is free — the
    scan admits pending rows regardless of cap — and even when the user or
    project switched automatic ticking off: a human asked for *this* run.
    ``fire_tick`` re-applies the switch after the claim so no timer tick
    follows on a disabled clock.
    """
    ticker.enabled = True
    ticker.disarm_reason = TickerDisarmReason.NONE
    ticker.next_run_at = timezone.now()
    ticker.pending_entry = True
    ticker.pending_entry_free = free
    ticker.pending_entry_actor = actor if free else None
    ticker.pending_entry_trigger = trigger if free else ""


def _stop_for_switch(ticker: IssueAgentTicker) -> None:
    _stop_clock(
        ticker,
        TickerDisarmReason.USER_DISABLED if ticker.user_disabled else TickerDisarmReason.NONE,
    )


def _retime_clock(ticker: IssueAgentTicker, issue: Issue) -> None:
    """Arm the clock for the current stage's interval if the pool allows.

    A spent pool stops the clock with ``POOL_SPENT`` (not ``CAP_HIT`` — that
    reason is reserved for the timer tick that consumed the last run, and it
    is the only one that auto-Pauses; a human-started run on a spent pool
    must leave the issue where the human can Re-tick it).
    """
    if not _clock_allowed(issue, ticker):
        _stop_for_switch(ticker)
        return
    if ticker.cap_reached():
        _stop_clock(ticker, TickerDisarmReason.POOL_SPENT)
        return
    ticker.enabled = True
    ticker.disarm_reason = TickerDisarmReason.NONE
    _clear_pending(ticker)
    ticker.next_run_at = _compute_next_run_at(ticker.effective_interval_seconds())


def normalize_outcome(value, phase_kind: str = "") -> Optional[str]:
    """Map a done-payload ``status`` onto the §7 outcome vocabulary.

    Returns ``None`` for anything unrecognised (a bridge's
    ``{"conclusion": …}`` payload has no status at all), which the
    ``RUN_ENDED`` handler treats as "the run did not yield". The legacy
    ``noop`` ("nothing changed") follows the per-kind default: an In
    Progress run that found nothing to do must keep ticking (CI may still
    be running), while a review / test that found nothing new is satisfied.
    """
    if not isinstance(value, str):
        return None
    value = value.strip().lower()
    if value in RUN_OUTCOMES:
        return value
    if value == "noop":
        return default_outcome_for_kind(phase_kind or "")
    return _LEGACY_OUTCOME_ALIASES.get(value)


def outcome_for_run(run: AgentRun) -> Optional[str]:
    payload = run.done_payload or {}
    if not isinstance(payload, dict):
        return None
    return normalize_outcome(payload.get("status"), getattr(run, "phase_kind", "") or "")


def default_outcome_for_kind(phase_kind: str) -> str:
    """What a run that exited without yielding is taken to mean.

    ``coding-task`` → keep ticking (the budget still bounds it). ``review``
    / ``test`` → ``done`` (stay; stop) — the cost-safe reading, so an
    approved review that forgets to yield does not tick to its cap.
    """
    from pi_dash.prompting.recipes import KIND_CODING_TASK

    if phase_kind in ("", KIND_CODING_TASK):
        return OUTCOME_PROGRESSED
    return OUTCOME_DONE


def reconcile(issue: Issue, event: TickerEvent) -> TickerDecision:
    """Apply one event to the issue's clock and say what the caller should do.

    Reads: the issue's current stage, ``used`` / ``granted``, the project's
    pool / grant / interval for the stage, the user and project switches,
    and whether a run is active. Writes: ``next_run_at``, ``pending_entry``,
    ``pending_entry_free``, ``enabled``, ``disarm_reason``, ``granted``,
    ``resume_parent_run``. Never writes ``used``.
    """
    handler = _EVENT_HANDLERS.get(event.kind)
    if handler is None:
        raise ValueError(f"unknown ticker event kind {event.kind!r}")
    with transaction.atomic():
        decision = handler(issue, event)
    logger.info(
        "agent_ticker: reconcile issue=%s event=%s -> dispatch_now=%s queued=%s parked=%s reason=%s",
        issue.pk,
        event.kind,
        decision.dispatch_now,
        decision.queued,
        decision.parked,
        decision.reason,
    )
    return decision


def _on_enter_or_move(issue: Issue, event: TickerEvent) -> TickerDecision:
    """Issue entered the bucket, or moved between its rooms (design §4.0).

    No teardown, no rebuild: the same row keeps ``used`` / ``granted``. The
    only questions are whether an entry run fires, whether it counts, and
    which interval the clock reads next.
    """
    ticker = _lock_ticker(issue, create=True)
    if event.resume_parent is not None:
        ticker.resume_parent_run = event.resume_parent

    decision = TickerDecision(ticker=ticker)
    agent_move = event.moved_by_run is not None
    clock_allowed = _clock_allowed(issue, ticker)

    if agent_move:
        if ticker.cap_reached():
            # Pool spent: nothing fires. The issue sits in its truthful
            # state; the run that moved it owes the human a comment (§5.4).
            # ``POOL_SPENT``, not ``CAP_HIT``: parking must not auto-Pause
            # the issue out from under the Re-tick the comment points at.
            _stop_clock(ticker, TickerDisarmReason.POOL_SPENT)
            decision.parked = True
            decision.reason = "pool-spent"
        elif not clock_allowed:
            _stop_for_switch(ticker)
            decision.reason = "ticking-disabled"
        else:
            # The agent moves the issue from inside its own run, so a
            # direct dispatch would hit the single-active-run guard. Queue
            # the entry on the clock; it counts (§5.2).
            _queue_entry(ticker, free=False)
            decision.queued = True
            decision.reason = "entry-queued"
    else:
        # Human move: one free run, always — even into a spent pool.
        if not event.want_run:
            _retime_clock(ticker, issue)
            decision.reason = "retimed"
        elif _issue_has_active_run(issue):
            _queue_entry(ticker, free=True, actor=event.actor, trigger=event.trigger)
            decision.queued = True
            decision.reason = "free-entry-queued"
        else:
            _retime_clock(ticker, issue)
            decision.dispatch_now = True
            decision.reason = "dispatch-now"
    _save_clock(ticker)
    return decision


def _on_left_bucket(issue: Issue, event: TickerEvent) -> TickerDecision:
    """Issue left the bucket: the clock goes dormant; the pool is kept."""
    ticker = _lock_ticker(issue, create=False)
    if ticker is None:
        return TickerDecision(reason="no-ticker")
    _stop_clock(ticker, TickerDisarmReason.LEFT_TICKING_STATE)
    ticker.next_run_at = None
    _save_clock(ticker)
    return TickerDecision(ticker=ticker, reason="dormant")


def _on_run_ended(issue: Issue, event: TickerEvent) -> TickerDecision:
    """A run on the issue reached a resting status (design §7).

    The guard: a run that moved the issue on and *then* reported ``done``
    must not stop the clock that is already set for the next stage.
    """
    run = event.run
    ticker = _lock_ticker(issue, create=False)
    if ticker is None:
        return TickerDecision(reason="no-ticker")
    state = issue.state
    if not is_ticking_state(state):
        return TickerDecision(ticker=ticker, reason="not-in-bucket")

    from pi_dash.orchestration.agent_phases import template_name_for
    from pi_dash.prompting.recipes import kind_for

    current_kind = kind_for(template_name_for(state))
    run_kind = getattr(run, "phase_kind", "") or ""
    if run_kind and run_kind != current_kind:
        return TickerDecision(ticker=ticker, reason="stage-moved-on")

    outcome = event.outcome if event.outcome is not None else outcome_for_run(run)
    if outcome is None:
        # No yield. A run that *completed* is read per kind (§7 defaults);
        # one that failed, was cancelled, or was refused said nothing about
        # the stage — keep ticking so the next tick retries, rather than
        # stopping the clock on a crash (which would strand a review/test
        # issue with no Re-tick button, since the cap was never reached).
        if run.status == AgentRunStatus.COMPLETED:
            outcome = default_outcome_for_kind(run_kind or current_kind)
        elif run.status == AgentRunStatus.PAUSED_AWAITING_INPUT:
            outcome = OUTCOME_WAITING_ON_HUMAN
        else:
            outcome = OUTCOME_PROGRESSED

    if ticker.pending_entry:
        # Something (a human, a queued hand-off) already owes the next run
        # on this clock; the finished run's opinion does not override it.
        return TickerDecision(ticker=ticker, reason=f"{outcome}:pending-entry-kept")

    if outcome in STOPPING_OUTCOMES:
        if ticker.enabled:
            # Only stop an armed clock — a prior ``cap_hit`` must survive so
            # the deferred auto-pause still fires (design §5.2).
            _stop_clock(ticker, TickerDisarmReason.TERMINAL_SIGNAL)
            _save_clock(ticker)
            return TickerDecision(ticker=ticker, reason=f"{outcome}:stopped")
        return TickerDecision(ticker=ticker, reason=f"{outcome}:already-stopped")

    # progressed / waiting_on_external: keep ticking. ``fire_tick`` already
    # re-timed the clock at claim for tick-started runs; make sure a
    # human-started run leaves a live clock behind too.
    if ticker.enabled and ticker.next_run_at is None and not ticker.cap_reached():
        ticker.next_run_at = _compute_next_run_at(ticker.effective_interval_seconds())
        _save_clock(ticker)
    return TickerDecision(ticker=ticker, reason=f"{outcome}:keep-ticking")


def _on_human_run_requested(issue: Issue, event: TickerEvent) -> TickerDecision:
    """Run AI / Comment & Run: one free run now; re-time only if budget."""
    ticker = _lock_ticker(issue, create=True)
    decision = TickerDecision(ticker=ticker)
    if event.want_run and _issue_has_active_run(issue):
        _queue_entry(ticker, free=True, actor=event.actor, trigger=event.trigger)
        decision.queued = True
        decision.reason = "free-entry-queued"
    else:
        _retime_clock(ticker, issue)
        decision.dispatch_now = event.want_run
        decision.reason = "dispatch-now" if event.want_run else "retimed"
    _save_clock(ticker)
    return decision


def _on_retick(issue: Issue, event: TickerEvent) -> TickerDecision:
    """Re-tick: grant one project-sized budget slice, then fire now.

    All guards must hold or the call is a no-op: a ticker row exists, the
    issue is in the bucket, the pool is actually spent.
    """
    ticker = _lock_ticker(issue, create=False)
    if ticker is None:
        return TickerDecision(reason="no_ticker")
    paused = is_paused_state(issue.state)
    if not is_ticking_state(issue.state) and not paused:
        return TickerDecision(ticker=ticker, reason="not_ticking_state")
    if not ticker.cap_reached():
        return TickerDecision(ticker=ticker, reason="budget_not_exhausted")

    grant = getattr(issue.project, "agent_retick_grant", DEFAULT_RETICK_GRANT)
    ticker.granted += max(0, int(grant))
    decision = TickerDecision(ticker=ticker, granted=True, reason="granted")
    if paused:
        # The cap-hit auto-pause parked the issue outside the bucket. The
        # grant lands here; ``re_tick_ticker`` moves the issue back to In
        # Progress as a human move, which arms the clock and fires the run.
        decision.reason = "granted-from-paused"
        _save_clock(ticker)
        return decision
    if event.want_run and _issue_has_active_run(issue):
        _queue_entry(ticker, free=True, actor=event.actor, trigger=event.trigger)
        decision.queued = True
    else:
        _retime_clock(ticker, issue)
        decision.dispatch_now = event.want_run
    _save_clock(ticker)
    return decision


_EVENT_HANDLERS = {
    TickerEventKind.ENTERED_BUCKET: _on_enter_or_move,
    TickerEventKind.MOVED_STAGE: _on_enter_or_move,
    TickerEventKind.LEFT_BUCKET: _on_left_bucket,
    TickerEventKind.RUN_ENDED: _on_run_ended,
    TickerEventKind.HUMAN_RUN_REQUESTED: _on_human_run_requested,
    TickerEventKind.RETICK: _on_retick,
}


# ---------------------------------------------------------------------------
# Thin senders — kept for callers and tests that predate ``reconcile``
# ---------------------------------------------------------------------------


def arm_ticker(
    issue: Issue,
    *,
    dispatch_immediate: bool = True,  # noqa: ARG001 — caller-only signal
) -> IssueAgentTicker:
    """Human re-engagement without a run: re-time the clock for the current
    stage if the pool allows. Never zeroes ``used``."""
    decision = reconcile(issue, TickerEvent.human_run_requested(want_run=False))
    return decision.ticker


def disarm_ticker(
    issue: Issue,
    *,
    reason: str = TickerDisarmReason.LEFT_TICKING_STATE,
) -> Optional[IssueAgentTicker]:
    """Stop the clock with ``reason``. Idempotent."""
    if reason == TickerDisarmReason.TERMINAL_SIGNAL:
        raise ValueError(
            "disarm_ticker overwrites disarm_reason — send a RUN_ENDED event "
            "(reconcile) for TERMINAL_SIGNAL so a prior CAP_HIT survives."
        )
    with transaction.atomic():
        ticker = _lock_ticker(issue, create=False)
        if ticker is None:
            return None
        _stop_clock(ticker, reason)
        if reason == TickerDisarmReason.LEFT_TICKING_STATE:
            ticker.next_run_at = None
        _save_clock(ticker)
    logger.info("agent_ticker: disarmed issue=%s reason=%s", issue.pk, reason)
    return ticker


def maybe_disarm_on_terminal_signal(run: AgentRun) -> bool:
    """Send ``RUN_ENDED`` for ``run``; ``True`` when the clock was stopped."""
    if run.work_item_id is None:
        return False
    decision = reconcile(run.work_item, TickerEvent.run_ended(run))
    return decision.reason.endswith(":stopped")


def reset_ticker_after_comment_and_run(issue: Issue) -> Optional[IssueAgentTicker]:
    """Comment & Run / Run AI, caller dispatches the run itself.

    Historical name — nothing is *reset* any more: human-started runs are
    free (design §5.2). Re-times the clock if the pool has budget.
    """
    decision = reconcile(issue, TickerEvent.human_run_requested(want_run=False))
    return decision.ticker


def re_tick_ticker(issue: Issue, *, actor=None) -> dict:
    """Grant a Re-tick and start a run now (design §5.5).

    Returns ``{"granted": bool, "reason": str, "ticker": IssueAgentTicker|None,
    "run": AgentRun|None}``.
    """
    from pi_dash.orchestration import service as orchestration_service

    # Grant, clock re-time and dispatch share one transaction: a Re-tick that
    # produced no run (no pod, no creator, a preflight bounce) is reported
    # as not granted and leaves the ticker exactly as it was.
    with transaction.atomic():
        locked = (
            Issue.all_objects.select_for_update(of=("self",))
            .select_related("state", "project", "workspace")
            .filter(pk=issue.pk)
            .first()
        )
        if locked is None:
            return {"granted": False, "reason": "no_issue", "ticker": None, "run": None}
        decision = reconcile(locked, TickerEvent.retick(actor=actor))
        result = {
            "granted": decision.granted,
            "reason": decision.reason,
            "ticker": decision.ticker,
            "run": None,
        }
        if not decision.granted:
            return result

        creator = actor if actor is not None else orchestration_service._resolve_fallback_creator(locked)
        if decision.reason == "granted-from-paused":
            # Bring the issue back into the bucket as a human move (free
            # entry, clock armed on the fresh budget). The signal only
            # updates the clock; we dispatch below so the run carries
            # ``actor``.
            target = _in_progress_state_for(locked)
            if target is None:
                transaction.set_rollback(True)
                return {"granted": False, "reason": "no_in_progress_state", "ticker": decision.ticker, "run": None}
            from pi_dash.orchestration.signals import _DISPATCH_IMMEDIATE_ATTR

            setattr(locked, _DISPATCH_IMMEDIATE_ATTR, False)
            locked.state = target
            if hasattr(locked, "updated_by") and creator is not None:
                locked.updated_by = creator
            locked.save(update_fields=["state", "updated_at"])
            decision.dispatch_now = True

        if decision.dispatch_now:
            run = dispatch_run_ai_run(locked, actor=creator) if creator is not None else None
            if run is None:
                transaction.set_rollback(True)
                return {"granted": False, "reason": "dispatch-failed", "ticker": decision.ticker, "run": None}
            result["run"] = run
            result["ticker"] = IssueAgentTicker.objects.filter(issue=locked).first()
    return result


def _in_progress_state_for(issue: Issue):
    """The project's registered In Progress state, if it has one."""
    from pi_dash.db.models.state import State, StateGroup
    from pi_dash.orchestration.agent_phases import PHASES

    cfg = PHASES.get(StateGroup.STARTED.value)
    if cfg is None:
        return None
    return (
        State.all_state_objects.filter(
            project_id=issue.project_id,
            group=StateGroup.STARTED.value,
            name=cfg.state_name,
            deleted_at__isnull=True,
        )
        .order_by("sequence")
        .first()
    )


# ---------------------------------------------------------------------------
# Continuation dispatch
# ---------------------------------------------------------------------------


# Sourced from ``AgentRunTrigger`` so a value change propagates here rather
# than silently diverging — these strings are passed straight to
# ``AgentRun.trigger`` and feed ``run_is_human_triggered`` (design §9.1).
TRIGGER_TICK = AgentRunTrigger.TICK.value
TRIGGER_COMMENT_AND_RUN = AgentRunTrigger.COMMENT_AND_RUN.value
TRIGGER_RUN_AI = AgentRunTrigger.RUN_AI.value


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
    """Resolve a current human execution principal; bots never own tool authority."""
    from pi_dash.core.agent_execution import AgentExecutorKind, effective_executor_for_issue

    effective = effective_executor_for_issue(issue)
    if effective == AgentExecutorKind.LOCAL_RUNNER:
        if actor is not None:
            return actor
        if triggered_by == TRIGGER_TICK:
            from pi_dash.orchestration.workpad import get_agent_system_user

            return get_agent_system_user()
        return issue.created_by or issue.project.project_lead or issue.project.default_assignee

    from pi_dash.core.permissions import ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER, check_project_role

    if actor is not None and triggered_by != TRIGGER_TICK:
        candidates = [actor]
    else:
        candidates = [issue.created_by, issue.project.project_lead, issue.project.default_assignee]
        # Live assignees only. ``Issue.assignees`` is a plain M2M over the
        # soft-deleted ``IssueAssignee`` through-model, so it would also offer
        # up users who were un-assigned — making a former assignee the
        # execution principal (and LLM-config owner) for a cloud run.
        from pi_dash.db.models.issue import IssueAssignee
        from pi_dash.db.models.user import User

        candidates.extend(
            User.objects.filter(
                pk__in=IssueAssignee.objects.filter(issue=issue).values_list("assignee_id", flat=True)
            ).order_by("id")
        )
    from pi_dash.core.agent_execution import user_has_llm_config

    seen = set()
    for candidate in candidates:
        candidate_id = getattr(candidate, "id", None)
        if candidate_id is None or candidate_id in seen:
            continue
        seen.add(candidate_id)
        if not getattr(candidate, "is_active", False) or getattr(candidate, "is_bot", False):
            continue
        # Cloud runs execute against the creator's LLM config, so the
        # execution principal must also hold a usable one (mirrors
        # dispatch_scheduler_run's candidate filter).
        if not user_has_llm_config(candidate):
            continue
        # A managed run additionally executes on the *creator's own machine*,
        # so a candidate with no desktop enrolled for this project could never
        # serve it — picking them would create a run that waits forever for a
        # laptop that does not exist. Note the system-user fallback above is
        # deliberately not reachable here: a bot has neither a desktop nor a
        # provider.
        if effective == AgentExecutorKind.MANAGED_RUNNER:
            from pi_dash.managed_runner.policy import enrolled_managed_runners

            if not enrolled_managed_runners(issue.project, candidate).exists():
                continue
        if check_project_role(
            candidate,
            issue.workspace.slug,
            issue.project_id,
            [ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST],
        ):
            return candidate
    return None


def preflight_eligibility_or_bounce(issue: Issue, *, run_creator, pod, triggered_by: str) -> bool:
    """Return True if dispatch can proceed; False if the issue was bounced.

    Companion preflight to the four issue-run dispatch paths. When no
    runner registered in ``pod`` has an owner the matcher's
    ``filter_runs_usable_by_runner`` would accept for a run on ``issue``
    created by ``run_creator``, this moves the issue back to its
    project's Backlog state and posts a system comment explaining why.
    The caller must NOT create the ``AgentRun`` when this returns False.

    See ``.ai_design/issue_runner/design.md`` §6.6.
    """
    from pi_dash.core.agent_execution import (
        AgentExecutorKind,
        effective_executor_for_issue,
        managed_runner_is_enabled,
        user_has_llm_config,
    )

    # Cloud capacity is independent of registered local machines, but the
    # execution principal must hold a usable LLM config — otherwise run
    # creation would raise CloudAgentUnavailable and the tick would be
    # swallowed with no user-visible signal. Bounce loudly instead.
    #
    # Branch on the issue's *effective* executor: an issue pinned to the Cloud
    # Agent on a local-default project has no pod requirement at all.
    effective = effective_executor_for_issue(issue)
    if effective == AgentExecutorKind.CLOUD_AGENT:
        if run_creator is not None and user_has_llm_config(run_creator):
            return True
        _bounce_issue_no_eligible_runner(issue, triggered_by=triggered_by, reason="no-llm-config")
        return False

    if effective == AgentExecutorKind.MANAGED_RUNNER:
        # Structural gates only. "Your laptop is closed right now" is not one
        # of them: the run is created and waits visibly (see
        # ``cloud_agent.creation._managed_execution_fields``), because bouncing
        # an issue back to Backlog every time a lid closes would be hostile and
        # would disarm the ticker for work that is merely paused.
        from pi_dash.managed_runner.errors import ManagedRunnerReason
        from pi_dash.managed_runner.policy import enrolled_managed_runners, managed_llm_profile

        if run_creator is None:
            _bounce_issue_no_eligible_runner(issue, triggered_by=triggered_by, reason="no-managed-runner")
            return False
        if not managed_runner_is_enabled():
            _bounce_issue_no_eligible_runner(
                issue, triggered_by=triggered_by, reason=ManagedRunnerReason.DISABLED
            )
            return False
        profile = managed_llm_profile(run_creator)
        if not profile.available:
            _bounce_issue_no_eligible_runner(
                issue,
                triggered_by=triggered_by,
                reason=profile.reason_code or ManagedRunnerReason.LLM_CONFIG_MISSING,
            )
            return False
        if not enrolled_managed_runners(issue.project, run_creator).exists():
            _bounce_issue_no_eligible_runner(issue, triggered_by=triggered_by, reason="no-managed-runner")
            return False
        return True

    from pi_dash.runner.services.matcher import (
        pod_has_runner_for_issue_principal,
    )

    creator_id = getattr(run_creator, "id", None)
    if pod_has_runner_for_issue_principal(pod, issue, creator_id):
        return True
    _bounce_issue_no_eligible_runner(issue, triggered_by=triggered_by)
    return False


def _bounce_issue_no_eligible_runner(issue: Issue, *, triggered_by: str, reason: str = "no-eligible-runner") -> None:
    """Move ``issue`` back to Backlog and post the no-eligible-runner notice.

    State move fires ``fire_state_transition`` which disarms the ticker as
    a side-effect (Backlog isn't a delegation trigger). Skipped when the
    issue is already in the BACKLOG state group — the comment still posts
    so the user sees *why* a click / tick produced no run.

    Target resolution (design §6.6 step 1): prefer the project's Backlog
    state (``default`` first, then ``sequence``); if the project has no
    Backlog state at all, fall back to ``project.default_state`` **only
    when it is not itself a ticking state** — moving into a ticking state
    would re-fire dispatch and bounce again in a loop. When neither a
    Backlog nor a safe fallback exists, the issue is left in place and the
    ticker is disarmed explicitly (below) so the next tick can't re-enter
    this bounce forever.

    The state move + comment post are wrapped in a single atomic block
    so a partial bounce (state changed, no comment) can't survive a
    crash mid-write — the user would otherwise be staring at an issue
    that silently jumped back to Backlog with no explanation.
    """
    from django.utils.html import format_html

    from pi_dash.db.models.issue import IssueComment
    from pi_dash.db.models.state import State, StateGroup
    from pi_dash.orchestration.workpad import get_agent_system_user

    logger.info(
        "agent_dispatch: bounce issue=%s reason=%s triggered_by=%s",
        issue.pk,
        reason,
        triggered_by,
    )

    if reason == "no-llm-config":
        body = format_html(
            "<p><strong>Agent run skipped — no AI provider configured.</strong></p>"
            "<p>This project uses the Pi Dash Cloud Agent, which runs against "
            "the triggering user's AI provider. Configure one in Pi Dash AI "
            "settings, or assign this issue to a member who has one configured.</p>"
        )
    else:
        body = format_html(
            "<p><strong>Agent run skipped — no eligible runner.</strong></p>"
            "<p>No runner is registered in this pod that can serve this issue. "
            "Add a runner under your account, or assign this issue to a "
            "workspace member whose runner is registered here.</p>"
        )

    with transaction.atomic():
        current_state_group = issue.state.group if issue.state_id else None
        if current_state_group != StateGroup.BACKLOG.value:
            target_state = (
                State.objects.filter(
                    project_id=issue.project_id,
                    group=StateGroup.BACKLOG.value,
                )
                .order_by("-default", "sequence")
                .first()
            )
            if target_state is None:
                # Defensive: DEFAULT_STATES seeds a Backlog state for every
                # project, but if one is missing fall back to the project's
                # default_state — only when it isn't itself a ticking state,
                # since moving into a ticking state re-fires dispatch and
                # re-bounces (design §6.6 step 1).
                fallback = issue.project.default_state
                if fallback is not None and not is_ticking_state(fallback):
                    target_state = fallback
                else:
                    logger.warning(
                        "agent_dispatch: no safe backlog target for "
                        "project=%s; issue=%s stays in current state, "
                        "ticker disarmed",
                        issue.project_id,
                        issue.pk,
                    )
            if target_state is not None and target_state.pk != issue.state_id:
                issue.state = target_state
                issue.save(update_fields=["state", "updated_at"])

        # If the issue couldn't be moved out of a ticking state, the
        # state-move signal never disarmed the ticker — do it explicitly so
        # a subsequent tick doesn't re-enter this bounce endlessly, spamming
        # a comment each time.
        if is_ticking_state(issue.state if issue.state_id else None):
            disarm_ticker(issue)

        IssueComment.objects.create(
            issue=issue,
            project=issue.project,
            workspace=issue.workspace,
            actor=get_agent_system_user(),
            comment_html=body,
            speaker_type=IssueComment.SpeakerType.AGENT,
        )


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
    guardrail blocks creation, no pod is available, or the eligibility
    preflight bounced the issue (§6.6).
    """
    from pi_dash.orchestration import service as orchestration_service

    if orchestration_service._active_run_for(issue) is not None:
        logger.info(
            "agent_ticker: skip dispatch issue=%s reason=active-run-exists triggered_by=%s",
            issue.pk,
            triggered_by,
        )
        return None

    if orchestration_service._latest_prior_run(issue) is None:
        logger.info(
            "agent_ticker: skip dispatch issue=%s reason=no-prior-run triggered_by=%s",
            issue.pk,
            triggered_by,
        )
        return None
    # The parent follows the same stage rules as a transition dispatch: a
    # queued entry into review / test starts a fresh session; a hand-back
    # into In Progress parents off the implementation lineage, not the
    # review run that sent it back.
    parent, fresh_session = orchestration_service.parent_for_next_run(issue)

    creator = _resolve_creator_for_trigger(issue, triggered_by=triggered_by, actor=actor)
    if creator is None:
        logger.warning(
            "agent_ticker: skip dispatch issue=%s reason=no-creator triggered_by=%s",
            issue.pk,
            triggered_by,
        )
        from pi_dash.core.agent_execution import AgentExecutorKind, effective_executor_for_issue

        effective = effective_executor_for_issue(issue)
        if effective == AgentExecutorKind.CLOUD_AGENT:
            # On a cloud project "no creator" means no candidate holds a
            # usable LLM config — bounce loudly instead of letting the
            # ticker fire forever with zero user-visible signal.
            _bounce_issue_no_eligible_runner(issue, triggered_by=triggered_by, reason="no-llm-config")
        elif effective == AgentExecutorKind.MANAGED_RUNNER:
            # A managed run belongs to one person's desktop, so "no creator"
            # means nobody could ever serve it — structural, bounce loudly.
            _bounce_issue_no_eligible_runner(issue, triggered_by=triggered_by, reason="no-managed-runner")
        return None

    pod = _resolve_pod_for_issue(issue)
    if pod is None:
        logger.warning(
            "agent_ticker: skip dispatch issue=%s reason=no-pod triggered_by=%s",
            issue.pk,
            triggered_by,
        )
        return None

    if not preflight_eligibility_or_bounce(issue, run_creator=creator, pod=pod, triggered_by=triggered_by):
        return None

    if fresh_session or parent is None:
        outcome = orchestration_service._create_and_dispatch_run(
            issue=issue,
            parent=None,
            creator=creator,
            pod=pod,
            fresh_session=True,
            trigger=triggered_by,
        )
    else:
        outcome = orchestration_service._create_continuation_run(
            issue=issue,
            parent=parent,
            creator=creator,
            pod=pod,
            trigger=triggered_by,
        )
    return outcome.created_run


def dispatch_run_ai_run(issue: Issue, *, actor) -> Optional[AgentRun]:
    """Public wrapper for the "Run AI" button.

    Builds the same templated prompt the state-transition-into-In-Progress
    path produces, by routing through the orchestration service's run-
    creation helpers (which call ``composer.build_first_turn``). This is
    the prompt-parity contract: a manual Run AI click renders the phase's
    template against the issue's current state, identical to a tick or a
    state transition into the same phase.

    Behavior:
    - Bails (returns ``None``) when an active run already exists on the
      issue (single-active-run guardrail) or no pod is available.
    - When a prior run exists, delegates to ``_create_continuation_run``
      so the new run inherits parent linkage and runner pinning (repo
      locality, same as Comment & Run / tick).
    - When no prior run exists, delegates to ``_create_and_dispatch_run``
      so a brand-new issue's first agent run still goes through the
      templated prompt path.
    """
    from pi_dash.orchestration import service as orchestration_service

    if orchestration_service._active_run_for(issue) is not None:
        logger.info(
            "agent_ticker: skip dispatch issue=%s reason=active-run-exists triggered_by=%s",
            issue.pk,
            TRIGGER_RUN_AI,
        )
        return None

    creator = _resolve_creator_for_trigger(issue, triggered_by=TRIGGER_RUN_AI, actor=actor)
    if creator is None:
        logger.warning(
            "agent_ticker: skip dispatch issue=%s reason=no-creator triggered_by=%s",
            issue.pk,
            TRIGGER_RUN_AI,
        )
        from pi_dash.core.agent_execution import AgentExecutorKind, effective_executor_for_issue

        effective = effective_executor_for_issue(issue)
        if effective == AgentExecutorKind.CLOUD_AGENT:
            _bounce_issue_no_eligible_runner(issue, triggered_by=TRIGGER_RUN_AI, reason="no-llm-config")
        elif effective == AgentExecutorKind.MANAGED_RUNNER:
            _bounce_issue_no_eligible_runner(issue, triggered_by=TRIGGER_RUN_AI, reason="no-managed-runner")
        return None

    pod = _resolve_pod_for_issue(issue)
    if pod is None:
        logger.warning(
            "agent_ticker: skip dispatch issue=%s reason=no-pod triggered_by=%s",
            issue.pk,
            TRIGGER_RUN_AI,
        )
        return None

    if not preflight_eligibility_or_bounce(issue, run_creator=creator, pod=pod, triggered_by=TRIGGER_RUN_AI):
        return None

    parent, fresh_session = orchestration_service.parent_for_next_run(issue)
    if parent is not None and not fresh_session:
        outcome = orchestration_service._create_continuation_run(
            issue=issue,
            parent=parent,
            creator=creator,
            pod=pod,
            trigger=TRIGGER_RUN_AI,
        )
    else:
        outcome = orchestration_service._create_and_dispatch_run(
            issue=issue,
            parent=None,
            creator=creator,
            pod=pod,
            fresh_session=fresh_session,
            trigger=TRIGGER_RUN_AI,
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

    Gated on ``disarm_reason == CAP_HIT`` — the timer tick that consumed
    the last run. Terminal-signal stops leave the issue in place for the
    human to act, and so does ``POOL_SPENT`` (an agent parked the issue, or
    a human moved it, on an already-spent pool): the §5.4 comment tells the
    human to Re-tick, which needs the issue to stay in the bucket.
    ``LEFT_TICKING_STATE`` and ``USER_DISABLED`` likewise are not
    auto-pause causes.

    Called from the runner Channels consumer after persisting a terminal
    run status.
    """
    if run.work_item_id is None:
        return False

    issue = run.work_item
    sched = IssueAgentTicker.objects.filter(issue=issue).first()
    if sched is None or sched.enabled:
        return False
    if sched.pending_entry:
        # A queued entry run (a human's free run, or a hand-off) will fire
        # as soon as the issue is free — do not pull the issue out from
        # under it.
        return False
    if sched.disarm_reason != TickerDisarmReason.CAP_HIT:
        return False

    state = issue.state
    if state is None:
        return False
    if not is_ticking_state(state):
        return False

    # The human-hand-off phases (In Review, In Test) are deliberately
    # excluded from the cap-hit auto-pause: when that budget is exhausted
    # the issue must simply stay put for a human to act — the runner never
    # promotes or reparks one on its own (PDASHOSS01-68 / PDASHOSS01-80).
    # This is registry-driven rather than a group check, so a new phase
    # declares its own answer instead of inheriting a silently-wrong
    # default here (design ``create_test_state/design.md`` §4.5). The
    # ticker is already disarmed above, so leaving the state untouched
    # does not resurrect ticking.
    from pi_dash.orchestration.agent_phases import auto_pauses_on_cap

    if not auto_pauses_on_cap(state):
        logger.info(
            "agent_ticker: %s cap hit for issue=%s — leaving it in place, "
            "no auto-pause",
            state.group,
            issue.pk,
        )
        return False

    # Other active runs (besides the one that just terminated) keep the
    # issue alive in In Progress — the next terminate will check again.
    from pi_dash.orchestration import service as orchestration_service

    active = orchestration_service._active_run_for(issue)
    if active is not None and active.pk != run.pk:
        return False

    paused_state = (
        type(state)
        .all_state_objects.filter(
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
        locked_sched = IssueAgentTicker.objects.select_for_update().filter(pk=sched.pk).first()
        if locked_sched is None or locked_sched.enabled:
            return False
        # Re-check disarm_reason under the lock so a concurrent
        # ``disarm_ticker`` (e.g., the user moved the issue out of the
        # ticking state mid-flight, flipping the reason from CAP_HIT to
        # LEFT_TICKING_STATE) cannot drive an auto-pause off a stale
        # unlocked read.
        if locked_sched.disarm_reason != TickerDisarmReason.CAP_HIT:
            return False
        # Re-fetch the issue under the same transaction to guard against a
        # racing state transition.
        IssueModel = type(issue)
        locked = IssueModel.all_objects.select_for_update().filter(pk=issue.pk).first()
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
    "DEFAULT_RETICK_GRANT",
    "DELEGATION_STATE_NAME",
    "INFINITE_MAX_TICKS",
    "OUTCOME_BLOCKED",
    "OUTCOME_DONE",
    "OUTCOME_PROGRESSED",
    "OUTCOME_WAITING_ON_EXTERNAL",
    "OUTCOME_WAITING_ON_HUMAN",
    "PAUSED_STATE_NAME",
    "RUN_OUTCOMES",
    "STOPPING_OUTCOMES",
    "TRIGGER_COMMENT_AND_RUN",
    "TRIGGER_RUN_AI",
    "TRIGGER_TICK",
    "TickerDecision",
    "TickerEvent",
    "TickerEventKind",
    "arm_ticker",
    "default_outcome_for_kind",
    "disarm_ticker",
    "dispatch_continuation_run",
    "is_paused_state",
    "dispatch_run_ai_run",
    "maybe_apply_deferred_pause",
    "maybe_disarm_on_terminal_signal",
    "normalize_outcome",
    "outcome_for_run",
    "re_tick_ticker",
    "reconcile",
    "reset_ticker_after_comment_and_run",
]
