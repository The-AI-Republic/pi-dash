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
#: Legacy done-payload statuses from the Cloud Agent structured result and
#: the pre-yield fence, mapped onto the §7 vocabulary. Outcomes are
#: informational only (PDASHOSS01-247) — none of them stops the clock; only
#: an explicit ``done_payload.stop_ticking`` does.
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
    ``noop`` ("nothing changed") follows the per-kind default —
    ``progressed`` in every stage since PDASHOSS01-247: outcomes are
    informational, and "nothing changed" never stops the clock.
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


def default_outcome_for_kind(phase_kind: str) -> str:  # noqa: ARG001 — kept for callers
    """What a run that exited without yielding is taken to mean.

    ``progressed`` in every stage: no report means keep ticking, and the
    budget bounds the cost (PDASHOSS01-247). Outcomes are informational
    only, so there is no cost-safe reason left to read a silent review /
    test run as ``done`` — that inference is what used to strand
    agent-driven issues with the clock off.
    """
    return OUTCOME_PROGRESSED


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

    The clock stops only on the run's explicit ``done_payload.stop_ticking``
    (``STOP_SIGNAL``). The outcome — including ``done``, ``blocked`` and
    ``waiting_on_human``, and a missing report — is informational and keeps
    the clock ticking; the budget bounds the cost (PDASHOSS01-247).

    The guard: a signal from a run whose stage the issue has already left
    is ignored — ``stop_ticking`` included — so the clock that is already
    set for the next stage survives.
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
        # No yield. The outcome is informational either way; keep ticking
        # so the next tick retries — whether the run completed silently,
        # paused on a question, failed, was cancelled, or was refused.
        if run.status == AgentRunStatus.PAUSED_AWAITING_INPUT:
            outcome = OUTCOME_WAITING_ON_HUMAN
        else:
            outcome = default_outcome_for_kind(run_kind or current_kind)

    if ticker.pending_entry:
        # Something (a human, a queued hand-off) already owes the next run
        # on this clock; the finished run's opinion does not override it.
        return TickerDecision(ticker=ticker, reason=f"{outcome}:pending-entry-kept")

    payload = run.done_payload if isinstance(run.done_payload, dict) else {}
    if payload.get("stop_ticking") is True:
        if ticker.enabled:
            # Only stop an armed clock — a prior ``cap_hit`` must survive so
            # the deferred auto-pause still fires (design §5.2).
            _stop_clock(ticker, TickerDisarmReason.STOP_SIGNAL)
            _save_clock(ticker)
            return TickerDecision(ticker=ticker, reason=f"{outcome}:stopped")
        return TickerDecision(ticker=ticker, reason=f"{outcome}:already-stopped")

    # No stop signal: keep ticking, whatever the outcome said. ``fire_tick``
    # already re-timed the clock at claim for tick-started runs; make sure a
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
    """Re-tick: grant a fresh project-sized pool, then fire now.

    The grant is one whole per-issue pool (``Project.agent_default_max_ticks``,
    default 10), so a press on a 10-pool issue takes the cap 10 → 20, a second
    press → 30; a 20-pool project grants 20 each press. One knob, not two.

    All guards must hold or the call is a no-op: a ticker row exists, the
    issue is in the bucket, the pool is actually spent. An infinite pool
    (``-1``) never reaches this grant — ``cap_reached()`` is never true so the
    ``budget_not_exhausted`` guard above returns first — but guard it anyway so
    a sentinel can never leak into ``granted``.
    """
    ticker = _lock_ticker(issue, create=False)
    if ticker is None:
        return TickerDecision(reason="no_ticker")
    paused = is_paused_state(issue.state)
    if not is_ticking_state(issue.state) and not paused:
        return TickerDecision(ticker=ticker, reason="not_ticking_state")
    if not ticker.cap_reached():
        return TickerDecision(ticker=ticker, reason="budget_not_exhausted")

    pool = ticker.pool_size()
    if pool == INFINITE_MAX_TICKS:
        # Unreachable given the cap guard above, but never fold the infinite
        # sentinel into ``granted``.
        return TickerDecision(ticker=ticker, reason="budget_not_exhausted")
    ticker.granted += max(0, int(pool))
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
    if reason in (TickerDisarmReason.TERMINAL_SIGNAL, TickerDisarmReason.STOP_SIGNAL):
        raise ValueError(
            "disarm_ticker overwrites disarm_reason — send a RUN_ENDED event "
            "(reconcile) for a run's stop signal so a prior CAP_HIT survives."
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
    """Send ``RUN_ENDED`` for ``run``; ``True`` when the clock was stopped.

    Historical name — since PDASHOSS01-247 only an explicit
    ``done_payload.stop_ticking`` stops the clock (``STOP_SIGNAL``), never
    the outcome itself.
    """
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
# The agent's wait — ``pidash issue wait`` (PDASHOSS01-204)
# ---------------------------------------------------------------------------
#
# The platform does not decide when a wait ends, and does not read the
# agent's workpad or the ``blocked_by`` semantics to guess that it started.
# Each tick the agent already sees its blockers and their states; if it
# decides it cannot safely proceed it says so here and yields. The only
# thing the platform does is make that decision free: one extra tick, so
# the run that is ending costs no net budget.


#: ``wait_ticker`` applied the wait.
WAIT_GRANTED = "waited"
#: The issue has spent its wait allowance (one extra pool). Further waiting
#: spends normal budget until the pool runs out and the issue parks.
WAIT_CAP_REACHED = "wait_cap_reached"
#: ``agent_default_max_ticks == -1``: there is no budget to buy back, so the
#: command is a no-op — as Re-tick already is on an infinite pool.
WAIT_INFINITE_POOL = "infinite_pool"
#: The issue has no clock (it never entered the ticking bucket), so there is
#: no budget to refund and nothing to wait for.
WAIT_NO_TICKER = "no_ticker"

#: ``IssueActivity.field`` recorded on every wait call.
WAIT_ACTIVITY_FIELD = "agent_wait"

#: Disarm reasons a wait may lift. Both mean "the pool ran out"; the extra
#: tick has nothing to buy if the clock never fires again.
_WAIT_REARMABLE_DISARMS = frozenset(
    {
        TickerDisarmReason.CAP_HIT,
        TickerDisarmReason.POOL_SPENT,
    }
)


def wait_ticker(issue: Issue, *, run: Optional[AgentRun] = None, actor=None) -> dict:
    """Buy back one tick so an agent can yield without spending budget.

    Returns ``{"applied": bool, "reason": str, "ticker": IssueAgentTicker|None}``.
    Every call adds a tick while the allowance lasts — including a second
    call inside the same run. The per-issue allowance (one extra pool) is the
    only limit; one run may spend the whole of it, and the cap bounds that.

    ``run`` is the run that asked, recorded on the activity entry so repeated
    waits are visible and attributable.
    """
    now = timezone.now()
    with transaction.atomic():
        ticker = (
            IssueAgentTicker.objects.select_for_update(of=("self",))
            .select_related("issue", "issue__state", "issue__project")
            .filter(issue_id=issue.pk)
            .first()
        )
        if ticker is None:
            return {"applied": False, "reason": WAIT_NO_TICKER, "ticker": None}
        if ticker.pool_size() == INFINITE_MAX_TICKS:
            return {"applied": False, "reason": WAIT_INFINITE_POOL, "ticker": ticker}
        if ticker.wait_allowance() <= 0:
            return {"applied": False, "reason": WAIT_CAP_REACHED, "ticker": ticker}

        ticker.waited = ticker.waited + 1
        update_fields = ["waited", "updated_at"]

        # Re-arm when the clock stopped *because the pool ran out* and the
        # raised cap has now left room. This is the common case, not an edge
        # one: an agent that waits on its last run has the claim disarm the
        # ticker (CAP_HIT) before it ever gets to call this, and a CAP_HIT
        # disarm is also what ``maybe_apply_deferred_pause`` reads to park
        # the issue when the run ends. Without the re-arm the refunded tick
        # would be unspendable and the issue would be Paused anyway.
        if (
            not ticker.enabled
            and ticker.disarm_reason in _WAIT_REARMABLE_DISARMS
            and not ticker.cap_reached()
            and not ticker.user_disabled
            and getattr(ticker.issue.project, "agent_ticking_enabled", True)
            and is_ticking_state(ticker.issue.state)
        ):
            interval = ticker.effective_interval_seconds()
            ticker.enabled = True
            ticker.disarm_reason = TickerDisarmReason.NONE
            ticker.next_run_at = now + timedelta(seconds=interval + jitter_seconds(interval))
            update_fields += ["enabled", "disarm_reason", "next_run_at"]

        ticker.save(update_fields=update_fields)
        _record_wait_activity(ticker.issue, ticker, run=run, actor=actor)

    logger.info(
        "agent_wait: issue=%s waited=%d used=%d cap=%d run=%s",
        issue.pk,
        ticker.waited,
        ticker.used,
        ticker.effective_max_ticks(),
        getattr(run, "pk", None),
    )
    return {"applied": True, "reason": WAIT_GRANTED, "ticker": ticker}


def _record_wait_activity(issue: Issue, ticker: IssueAgentTicker, *, run=None, actor=None) -> None:
    """Log the wait on the issue's activity feed, naming the calling run."""
    import time

    from pi_dash.db.models.issue import IssueActivity
    from pi_dash.orchestration.workpad import get_agent_system_user

    pool = ticker.pool_size()
    run_id = str(run.pk) if run is not None else ""
    IssueActivity.objects.create(
        issue=issue,
        project_id=issue.project_id,
        workspace_id=issue.workspace_id,
        verb="updated",
        field=WAIT_ACTIVITY_FIELD,
        # The allowance and the wait's position in it, so the UI can render
        # "waited (3 of 10)" without re-deriving project policy.
        old_value=str(pool),
        new_value=str(ticker.waited),
        comment=f"Waited on a blocker ({ticker.waited} of {pool})" + (f"; run {run_id}" if run_id else ""),
        actor=actor if actor is not None else get_agent_system_user(),
        epoch=time.time(),
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
#: Triggers the clock starts on its own — resolved like a tick (system bot on
#: a local runner, never an explicit actor).
_MACHINE_TRIGGERS = frozenset({TRIGGER_TICK})


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
    from pi_dash.core.agent_execution import MACHINE_EXECUTORS, effective_executor_for_issue

    effective = effective_executor_for_issue(issue)
    if effective in MACHINE_EXECUTORS:
        if actor is not None:
            return actor
        if triggered_by in _MACHINE_TRIGGERS:
            from pi_dash.orchestration.workpad import get_agent_system_user

            return get_agent_system_user()
        return issue.created_by or issue.project.project_lead or issue.project.default_assignee

    from pi_dash.core.permissions import ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER, check_project_role

    if actor is not None and triggered_by not in _MACHINE_TRIGGERS:
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

    # Every other executor value is a runner (design ``built_engine_as_runner``
    # §6; a stale ``managed_runner`` value is the legacy spelling of one). The
    # engine usage rule depends on whether a person or the clock started the
    # run, so the structural check is told which (§8). "The runner is offline
    # right now" is never a reason to bounce: the run waits in the pod.
    from pi_dash.runner.models import HUMAN_TRIGGERS
    from pi_dash.runner.services.matcher import (
        pod_has_runner_for_issue_principal,
    )

    creator_id = getattr(run_creator, "id", None)
    human = triggered_by in HUMAN_TRIGGERS
    if pod_has_runner_for_issue_principal(pod, issue, creator_id, human=human):
        return True
    # §9.4: a dedicated issue bounced for usage-rule exclusion reports
    # dedicated-runner-not-eligible and names the runner + failing rule.
    if getattr(issue, "assigned_runner_id", None) is not None:
        _bounce_issue_no_eligible_runner(issue, triggered_by=triggered_by, reason="dedicated-runner-not-eligible")
    else:
        _bounce_issue_no_eligible_runner(issue, triggered_by=triggered_by)
    return False


def _post_dedicated_refusal_comment(issue: Issue, *, code: str, actor=None) -> None:
    """One system comment for a refused human run (design §11.2).

    The state move stands (or the Comment & Run comment is saved); no run
    starts. The clock was never armed: the refusal check ran before
    ``reconcile``. The comment names the runner, why, and the two options.
    """
    from django.utils.html import format_html

    from pi_dash.db.models.issue import IssueComment
    from pi_dash.orchestration.workpad import get_agent_system_user
    from pi_dash.runner.services.issue_assignment import dedicated_refusal_body

    body_text = dedicated_refusal_body(issue, code=code)["error"]
    pod = getattr(issue, "assigned_pod", None)
    pod_name = pod.name if pod is not None else "the pod"
    if code == "assigned_runner_not_owner":
        options = (
            f"Run on my runner instead, or Run on any runner in {pod_name} once "
            "(sends \\u201cpin\\u201d: \\u201cpod_once\\u201d)."
        )
    else:
        options = (
            f"Run on any runner in {pod_name} once "
            "(sends \\u201cpin\\u201d: \\u201cpod_once\\u201d), or change the issue's runner."
        )
    body = format_html(
        "<p>No run started: {}.</p><p>{}</p>",
        body_text,
        options,
    )
    IssueComment.objects.create(
        issue=issue,
        project=issue.project,
        workspace=issue.workspace,
        actor=actor or get_agent_system_user(),
        comment_html=body,
        speaker_type=IssueComment.SpeakerType.SYSTEM,
    )


def _post_pod_once_started_comment(issue: Issue, *, pod, actor) -> None:
    """One system comment for a successful ``pod_once`` run (§23.3)."""
    from django.utils.html import format_html

    from pi_dash.db.models.issue import IssueComment
    from pi_dash.orchestration.workpad import get_agent_system_user

    runner_name = getattr(getattr(issue, "assigned_runner", None), "name", "the dedicated runner")
    actor_name = getattr(actor, "display_name", None) or getattr(actor, "username", None) or "a member"
    body = format_html(
        "<p>Run started on any runner in pod <strong>{}</strong> by {}, "
        "for this run only. The issue stays assigned to <strong>{}</strong>.</p>",
        pod.name if pod is not None else "the pod",
        actor_name,
        runner_name,
    )
    IssueComment.objects.create(
        issue=issue,
        project=issue.project,
        workspace=issue.workspace,
        actor=actor or get_agent_system_user(),
        comment_html=body,
        speaker_type=IssueComment.SpeakerType.SYSTEM,
    )


def _post_dedicated_waiting_comment(issue: Issue, *, run, runner) -> None:
    """One system comment for an automatic run waiting on an unavailable runner (§9.4)."""
    from django.conf import settings

    from django.utils.html import format_html

    from pi_dash.db.models.issue import IssueComment
    from pi_dash.orchestration.workpad import get_agent_system_user
    from pi_dash.runner.services.issue_assignment import (
        runner_is_offline,
        runner_paused_reason,
        refusal_pause_phrase,
    )

    pause = runner_paused_reason(runner)
    pod = getattr(issue, "assigned_pod", None) or getattr(run, "pod", None)
    pod_name = pod.name if pod is not None else "the pod"
    if pause is not None:
        detail = refusal_pause_phrase(pause)
    elif runner_is_offline(runner):
        seen = runner.last_heartbeat_at.strftime("%H:%M") if runner.last_heartbeat_at else "unknown"
        detail = f"is offline (last seen {seen})"
    else:
        detail = "is unavailable"
    expires = ""
    if run.created_at is not None:
        expires_at = run.created_at + timedelta(seconds=settings.DEDICATED_RUN_QUEUED_MAX_AGE_SECS)
        expires = format_html(
            " If it has not started by {} (12 hours), it fails and automatic runs on this issue stop.",
            expires_at.strftime("%H:%M"),
        )
    body = format_html(
        "<p>Queued for runner <strong>{}</strong>, which {}. "
        "The run starts when it reconnects.{} "
        "To run it elsewhere, cancel it and use <strong>Run on any runner in {} once</strong>, "
        "or change the issue's runner.</p>",
        runner.name,
        detail,
        expires,
        pod_name,
    )
    IssueComment.objects.create(
        issue=issue,
        project=issue.project,
        workspace=issue.workspace,
        actor=get_agent_system_user(),
        comment_html=body,
        speaker_type=IssueComment.SpeakerType.SYSTEM,
    )


def _bounce_issue_no_eligible_runner(
    issue: Issue, *, triggered_by: str, reason: str = "no-eligible-runner", reason_name: str = ""
) -> None:
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
    elif reason == "dedicated-runner-not-eligible":
        assigned_name = getattr(getattr(issue, "assigned_runner", None), "name", "the dedicated runner")
        body = format_html(
            "<p><strong>Agent run skipped — dedicated runner not eligible.</strong></p>"
            "<p>Runner <strong>{}</strong> cannot serve this run: its usage rule "
            "excludes the run's creator or trigger. Reassign the issue to a "
            "runner that can serve it, or return it to the pod.</p>",
            assigned_name,
        )
    elif reason == "dedicated-runner-revoked":
        body = format_html(
            "<p><strong>Dedicated runner lost.</strong></p>"
            "<p>Runner <strong>{}</strong> was revoked or deleted, so this issue "
            "is no longer assigned to a dedicated runner. Choose a new runner "
            "or return the issue to the pod.</p>",
            reason_name,
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
    pod_once: bool = False,
    held_entry: bool = False,
) -> Optional[AgentRun]:
    """Public wrapper for tick / Comment & Run dispatch.

    Resolves parent (latest prior run), creator (system bot for ticks;
    explicit ``actor`` for Comment & Run), pod, then delegates to
    :func:`pi_dash.orchestration.service._create_continuation_run`.
    Returns the created run, or ``None`` when the single-active-run
    guardrail blocks creation, no pod is available, or the eligibility
    preflight bounced the issue (§6.6).

    A person's run on an offline, paused or usage-excluded dedicated
    runner is refused here, before any reconcile or persistence (H-11,
    H-10, §11, §23.3): no run is created and the issue stays where it is.
    An automatic run waits. ``pod_once`` (§23.3) deliberately bypasses
    the dedicated runner for this one run only; the tick never passes it.
    """
    from pi_dash.orchestration import service as orchestration_service
    from pi_dash.runner.models import HUMAN_TRIGGERS
    from pi_dash.runner.services.issue_assignment import (
        dedicated_run_refusal,
        pod_once_auth_code,
    )

    human = triggered_by in HUMAN_TRIGGERS
    # A held entry (§11.2) already passed the check when the person asked;
    # if the runner became unavailable since, the run is created and waits
    # as an automatic one does. Skip the refusal, keep the preflight.
    if held_entry:
        pass
    elif human and pod_once:
        auth = pod_once_auth_code(issue, user=actor)
        if auth is not None:
            logger.info(
                "agent_ticker: skip dispatch issue=%s reason=%s triggered_by=%s",
                issue.pk,
                auth,
                triggered_by,
            )
            return None
    elif human:
        code, _detail = dedicated_run_refusal(issue, human=True, user=actor, pod_once=pod_once)
        if code is not None:
            logger.info(
                "agent_ticker: skip dispatch issue=%s reason=%s triggered_by=%s",
                issue.pk,
                code,
                triggered_by,
            )
            return None

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
        return None

    pod = _resolve_pod_for_issue(issue)
    if pod is None:
        logger.warning(
            "agent_ticker: skip dispatch issue=%s reason=no-pod triggered_by=%s",
            issue.pk,
            triggered_by,
        )
        return None

    if pod_once:
        # §23.3: pod-branch preflight, no bounce — the issue stays dedicated.
        from pi_dash.runner.services.matcher import pod_has_runner_for_issue_principal

        if not pod_has_runner_for_issue_principal(pod, issue, getattr(creator, "id", None), human=human, pod_once=True):
            logger.info(
                "agent_ticker: skip dispatch issue=%s reason=%s triggered_by=%s",
                issue.pk,
                RUN_AI_NO_ELIGIBLE_RUNNER_IN_POD,
                triggered_by,
            )
            return None
    elif not preflight_eligibility_or_bounce(issue, run_creator=creator, pod=pod, triggered_by=triggered_by):
        return None

    if fresh_session or parent is None:
        outcome = orchestration_service._create_and_dispatch_run(
            issue=issue,
            parent=None,
            creator=creator,
            pod=pod,
            fresh_session=True,
            trigger=triggered_by,
            pod_once=pod_once,
        )
    else:
        outcome = orchestration_service._create_continuation_run(
            issue=issue,
            parent=parent,
            creator=creator,
            pod=pod,
            trigger=triggered_by,
            pod_once=pod_once,
        )
    run = outcome.created_run
    if run is not None and pod_once:
        _post_pod_once_started_comment(issue, pod=pod, actor=actor or creator)
    return run


# Machine-readable refusal reasons for a "Run AI" dispatch that produced no
# run. The token ``run-ai`` endpoint surfaces these so a CLI / MCP caller can
# tell "already running or queued" (retry later) apart from "nothing can run
# it" (a structural problem to fix).
RUN_AI_ACTIVE_RUN_EXISTS = "active_run_exists"
RUN_AI_NO_POD = "no_pod"
RUN_AI_NO_ELIGIBLE_RUNNER = "no_eligible_runner"
RUN_AI_DEDICATED_RUNNER_OFFLINE = "assigned_runner_offline"
RUN_AI_DEDICATED_RUNNER_PAUSED = "assigned_runner_paused"
RUN_AI_DEDICATED_NOT_OWNER = "assigned_runner_not_owner"
RUN_AI_POD_ONCE_NOT_ALLOWED = "pod_once_not_allowed"
RUN_AI_POD_ONCE_NOT_APPLICABLE = "pod_once_not_applicable"
RUN_AI_NO_ELIGIBLE_RUNNER_IN_POD = "no_eligible_runner_in_pod"


def dispatch_run_ai_run(issue: Issue, *, actor, pod_once: bool = False) -> Optional[AgentRun]:
    """Public wrapper for the "Run AI" button, returning just the run.

    Thin adapter over :func:`dispatch_run_ai_run_with_reason` for callers
    that only care whether a run was created (the web ``_post_run_ai`` path
    and ``re_tick_ticker``). See that function for the prompt-parity
    contract and the refusal-reason vocabulary.
    """
    run, _reason = dispatch_run_ai_run_with_reason(issue, actor=actor, pod_once=pod_once)
    return run


def dispatch_run_ai_run_with_reason(
    issue: Issue, *, actor, pod_once: bool = False
) -> tuple[Optional[AgentRun], Optional[str]]:
    """"Run AI" dispatch that also reports *why* nothing was created.

    Builds the same templated prompt the state-transition-into-In-Progress
    path produces, by routing through the orchestration service's run-
    creation helpers (which call ``composer.build_first_turn``). This is
    the prompt-parity contract: a manual Run AI click renders the phase's
    template against the issue's current state, identical to a tick or a
    state transition into the same phase.

    Behavior:
    - Returns ``(None, RUN_AI_ACTIVE_RUN_EXISTS)`` when an active run
      already exists on the issue (single-active-run guardrail).
    - Returns ``(None, RUN_AI_NO_ELIGIBLE_RUNNER)`` when no execution
      principal / eligible runner can serve the run (no creator, or the
      eligibility preflight bounced the issue).
    - Returns ``(None, RUN_AI_NO_POD)`` when no pod is available.
    - Returns ``(None, RUN_AI_DEDICATED_RUNNER_OFFLINE/PAUSED/NOT_OWNER)``
      when the dedicated runner refuses the run (H-11, H-10, §11, §23.3):
      no run is created and the issue stays where it is. Busy is never
      a refusal. Skipped on ``pod_once``.
    - Returns ``(None, RUN_AI_POD_ONCE_*)`` / ``RUN_AI_NO_ELIGIBLE_RUNNER_IN_POD``
      for ``pod_once`` authorization / pod preflight failures (§23.3).
    - When a prior run exists, delegates to ``_create_continuation_run``
      so the new run inherits parent linkage and runner pinning (repo
      locality, same as Comment & Run / tick).
    - When no prior run exists, delegates to ``_create_and_dispatch_run``
      so a brand-new issue's first agent run still goes through the
      templated prompt path.
    - ``pod_once=True`` sends the run to the pod for this run only (§23.3).

    On success returns ``(run, None)``.
    """
    from pi_dash.orchestration import service as orchestration_service
    from pi_dash.runner.services.issue_assignment import (
        dedicated_run_refusal,
        pod_once_auth_code,
    )

    # H-11/H-10 refusal, before reconcile/persistence: a person's run on an
    # offline, paused or usage-excluded dedicated runner is refused. Skipped
    # on pod_once, which is authorized separately (§23.3).
    if pod_once:
        auth = pod_once_auth_code(issue, user=actor)
        if auth is not None:
            logger.info(
                "agent_ticker: skip dispatch issue=%s reason=%s triggered_by=%s",
                issue.pk,
                auth,
                TRIGGER_RUN_AI,
            )
            return None, auth
    else:
        refusal, _detail = dedicated_run_refusal(issue, human=True, user=actor, pod_once=pod_once)
        if refusal is not None:
            logger.info(
                "agent_ticker: skip dispatch issue=%s reason=%s triggered_by=%s",
                issue.pk,
                refusal,
                TRIGGER_RUN_AI,
            )
            return None, refusal

    if orchestration_service._active_run_for(issue) is not None:
        logger.info(
            "agent_ticker: skip dispatch issue=%s reason=active-run-exists triggered_by=%s",
            issue.pk,
            TRIGGER_RUN_AI,
        )
        return None, RUN_AI_ACTIVE_RUN_EXISTS

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
        return None, RUN_AI_NO_ELIGIBLE_RUNNER

    pod = _resolve_pod_for_issue(issue)
    if pod is None:
        logger.warning(
            "agent_ticker: skip dispatch issue=%s reason=no-pod triggered_by=%s",
            issue.pk,
            TRIGGER_RUN_AI,
        )
        return None, RUN_AI_NO_POD

    if pod_once:
        from pi_dash.runner.services.matcher import pod_has_runner_for_issue_principal

        if not pod_has_runner_for_issue_principal(pod, issue, getattr(creator, "id", None), human=True, pod_once=True):
            logger.info(
                "agent_ticker: skip dispatch issue=%s reason=%s triggered_by=%s",
                issue.pk,
                RUN_AI_NO_ELIGIBLE_RUNNER_IN_POD,
                TRIGGER_RUN_AI,
            )
            return None, RUN_AI_NO_ELIGIBLE_RUNNER_IN_POD
    elif not preflight_eligibility_or_bounce(issue, run_creator=creator, pod=pod, triggered_by=TRIGGER_RUN_AI):
        return None, RUN_AI_NO_ELIGIBLE_RUNNER

    parent, fresh_session = orchestration_service.parent_for_next_run(issue)
    if parent is not None and not fresh_session:
        outcome = orchestration_service._create_continuation_run(
            issue=issue,
            parent=parent,
            creator=creator,
            pod=pod,
            trigger=TRIGGER_RUN_AI,
            pod_once=pod_once,
        )
    else:
        outcome = orchestration_service._create_and_dispatch_run(
            issue=issue,
            parent=None,
            creator=creator,
            pod=pod,
            fresh_session=fresh_session,
            trigger=TRIGGER_RUN_AI,
            pod_once=pod_once,
        )
    run = outcome.created_run
    if run is not None and pod_once:
        _post_pod_once_started_comment(issue, pod=pod, actor=actor)
    return run, None


def run_ai_for_human(issue: Issue, *, actor, pod_once: bool = False) -> tuple[Optional[AgentRun], Optional[str]]:
    """Shared body for a human-initiated "Run AI" over the token API.

    Re-times the clock for the free human run, dispatches through the same
    :func:`dispatch_run_ai_run_with_reason` the web button uses, and rolls
    the re-time back when nothing was created — so a refused call leaves the
    ticker exactly as it was (design §5.2, mirrors ``_post_run_ai``'s
    rollback). Returns ``(run, reason)``; ``reason`` is a machine-readable
    refusal code when ``run`` is ``None``.
    """
    with transaction.atomic():
        reset_ticker_after_comment_and_run(issue)
        run, reason = dispatch_run_ai_run_with_reason(issue, actor=actor, pod_once=pod_once)
        if run is None:
            transaction.set_rollback(True)
    return run, reason



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
    the last run. Stop-signal stops (and legacy terminal-signal rows) leave
    the issue in place for the human to act, and so does ``POOL_SPENT``
    (an agent parked the issue, or
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
    "DELEGATION_STATE_NAME",
    "INFINITE_MAX_TICKS",
    "OUTCOME_BLOCKED",
    "OUTCOME_DONE",
    "OUTCOME_PROGRESSED",
    "OUTCOME_WAITING_ON_EXTERNAL",
    "OUTCOME_WAITING_ON_HUMAN",
    "PAUSED_STATE_NAME",
    "RUN_AI_ACTIVE_RUN_EXISTS",
    "RUN_AI_NO_ELIGIBLE_RUNNER",
    "RUN_AI_NO_POD",
    "RUN_OUTCOMES",
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
    "dispatch_run_ai_run_with_reason",
    "run_ai_for_human",
    "maybe_apply_deferred_pause",
    "maybe_disarm_on_terminal_signal",
    "normalize_outcome",
    "outcome_for_run",
    "re_tick_ticker",
    "reconcile",
    "reset_ticker_after_comment_and_run",
    "WAIT_ACTIVITY_FIELD",
    "WAIT_CAP_REACHED",
    "WAIT_GRANTED",
    "WAIT_INFINITE_POOL",
    "WAIT_NO_TICKER",
    "wait_ticker",
]
