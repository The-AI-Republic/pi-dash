# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-issue agent ticker — one clock per issue.

The continuation clock that re-invokes the agent on an issue while it sits
in the *ticking bucket* (In Progress / In Review / In Test). Internal
lifecycle machinery, system-armed on Issue state transitions; it is not a
user-authored periodic task.

There is exactly one row per issue and it is **never torn down and rebuilt**
when the issue moves between the three ticking stages: moving between rooms
of the bucket is a parameter change (which interval the clock reads, which
prompt the run gets), not a re-arm. Budget is one pool per issue — ``used``
counts every machine-started run in any stage for the life of the issue;
``granted`` is the extra budget a human added with Re-tick.

See ``.ai_design/ticking_relevance/design.md`` §4.0, §5 and §9.
"""

from __future__ import annotations

import random

from django.db import models

from .base import BaseModel
from .issue import Issue


#: Registry-level fallbacks, used only when the project row somehow lacks
#: the field (``getattr`` default). Project defaults are the real policy.
DEFAULT_INTERVAL_SECONDS = 43200  # 12 h
DEFAULT_MAX_TICKS = 10            # one pool per issue, any stage
DEFAULT_RETICK_GRANT = 3
INFINITE_MAX_TICKS = -1
JITTER_FRACTION = 0.1


class TickerDisarmReason(models.TextChoices):
    """Why the ticker is currently disarmed.

    ``maybe_apply_deferred_pause`` only auto-Pauses the issue when
    ``disarm_reason == CAP_HIT``. Terminal-signal disarms (``done`` /
    ``blocked`` / ``waiting_on_human``) leave the issue in place for the
    human to act. See ``.ai_design/ticking_relevance/design.md`` §5.2 / §7.
    """

    NONE = "", "None"
    LEFT_TICKING_STATE = "left_ticking_state", "Left Ticking State"
    #: A timer tick consumed the last run in the pool. The only reason that
    #: auto-Pauses an In Progress issue (``maybe_apply_deferred_pause``).
    CAP_HIT = "cap_hit", "Cap Hit"
    #: The pool was already spent when the issue moved (an agent parked it
    #: in its truthful state, or a human moved it and got one free run).
    #: Never auto-pauses — the human is expected to Re-tick from here, and
    #: Re-tick needs the issue to stay in the bucket.
    POOL_SPENT = "pool_spent", "Pool Spent"
    TERMINAL_SIGNAL = "terminal_signal", "Terminal Signal"
    USER_DISABLED = "user_disabled", "User Disabled"


def jitter_seconds(interval_seconds: int) -> float:
    """Uniform random offset in ``[0, interval × JITTER_FRACTION)``.

    Spreads out tick fires so that bulk transitions (e.g. sprint planning
    moving 50 issues to In Progress at once) do not re-cluster every cycle.
    """
    if interval_seconds <= 0:
        return 0.0
    return random.uniform(0, interval_seconds * JITTER_FRACTION)


class IssueAgentTicker(BaseModel):
    """The clock that drives periodic agent re-invocation for one issue.

    Exactly one row per issue (``issue`` is unique). Every event that can
    change the clock goes through ``orchestration.scheduling.reconcile``;
    nothing else should write these fields directly except ``fire_tick``'s
    claim (which is the only writer of ``used``).
    """

    issue = models.OneToOneField(
        Issue,
        on_delete=models.CASCADE,
        related_name="agent_ticker",
    )

    # ------------------------------------------------------------------
    # Budget — one pool for the life of the issue
    # ------------------------------------------------------------------
    #: Machine-started runs consumed so far, in any stage. Never reset on a
    #: stage change or on re-entry to the bucket. Only ``fire_tick`` writes
    #: it. Human-started runs (a human moving the issue, Comment & Run, Run
    #: AI) do not touch it.
    used = models.IntegerField(default=0)
    #: Extra budget added by Re-tick. Cap = project pool + ``granted``.
    granted = models.IntegerField(default=0)

    user_disabled = models.BooleanField(default=False)

    # ------------------------------------------------------------------
    # The clock
    # ------------------------------------------------------------------
    next_run_at = models.DateTimeField(null=True, blank=True)
    last_tick_at = models.DateTimeField(null=True, blank=True)
    #: Persisted form of the derived "is the clock live" answer —
    #: ``reconcile`` recomputes it on every event (see
    #: ``scheduling._enabled_for``). Stored, not computed, so the scanner
    #: can index on it.
    enabled = models.BooleanField(default=True)
    #: Why the ticker is currently disarmed. Empty string when armed.
    #: Load-bearing for the cap-hit-only auto-pause gate in
    #: ``maybe_apply_deferred_pause``.
    disarm_reason = models.CharField(
        max_length=32,
        blank=True,
        default="",
        choices=TickerDisarmReason.choices,
    )
    #: An entry run for the current stage is owed but could not be created
    #: because a run was active at the time (design §4.5). ``next_run_at``
    #: is already ``now``; ``fire_tick`` fires it as soon as the issue is
    #: free and clears the flag. The UI reads it as "next run queued".
    pending_entry = models.BooleanField(default=False)
    #: The pending entry was human-started (a human moved the issue,
    #: Comment & Run, Run AI, Re-tick) and therefore must not count against
    #: the pool when ``fire_tick`` claims it.
    pending_entry_free = models.BooleanField(default=False)
    #: Who asked for the pending entry (a human lever), so the queued run is
    #: created as that person — same LLM config / runner eligibility as if
    #: it had dispatched immediately. ``None`` for an agent-queued entry.
    pending_entry_actor = models.ForeignKey(
        "db.User",
        null=True,
        blank=True,
        on_delete=models.SET_NULL,
        related_name="+",
    )
    #: The ``AgentRunTrigger`` the pending entry should be created with
    #: (``run_ai`` / ``comment_and_run`` / ``state_transition``); empty for
    #: an agent-queued entry, which fires as a ``tick``.
    pending_entry_trigger = models.CharField(max_length=24, blank=True, default="")

    #: The latest implementation-phase run, captured on every cross-stage
    #: move so a hand-back to In Progress can parent off the implementation
    #: lineage rather than off a review/test run.
    resume_parent_run = models.ForeignKey(
        "runner.AgentRun",
        null=True,
        blank=True,
        on_delete=models.SET_NULL,
        related_name="+",
    )

    class Meta:
        db_table = "issue_agent_ticker"
        verbose_name = "Issue Agent Ticker"
        verbose_name_plural = "Issue Agent Tickers"
        indexes = [
            models.Index(
                fields=["enabled", "next_run_at"],
                name="iaticker_enabled_next_run_idx",
            ),
        ]

    def __str__(self) -> str:
        return f"IssueAgentTicker(issue={self.issue_id}, enabled={self.enabled})"

    # ------------------------------------------------------------------
    # Effective values
    # ------------------------------------------------------------------

    def effective_interval_seconds(self) -> int:
        """Interval for the issue's *current* stage — project policy.

        Cadence is rhythm, not budget: it may differ per stage (12 h / 8 h /
        12 h) even though the budget is one pool.
        """
        # Local import keeps the model file free of orchestration imports
        # at module load time (orchestration imports state).
        from pi_dash.orchestration.agent_phases import cadence_fields_for

        fields = cadence_fields_for(self.issue.state)
        return getattr(self.issue.project, fields.project_interval, fields.default_interval)

    def pool_size(self) -> int:
        """The project's per-issue pool, before any Re-tick grant.
        ``-1`` means infinite."""
        return getattr(self.issue.project, "agent_default_max_ticks", DEFAULT_MAX_TICKS)

    def effective_max_ticks(self) -> int:
        """Cap = project pool + ``granted``. ``-1`` means infinite."""
        pool = self.pool_size()
        if pool == INFINITE_MAX_TICKS:
            return INFINITE_MAX_TICKS
        return pool + self.granted

    def remaining(self) -> int | None:
        """Runs left in the pool, or ``None`` when the cap is infinite."""
        cap = self.effective_max_ticks()
        if cap == INFINITE_MAX_TICKS:
            return None
        return max(0, cap - self.used)

    def cap_reached(self) -> bool:
        """Has this issue exhausted its pool?"""
        cap = self.effective_max_ticks()
        if cap == INFINITE_MAX_TICKS:
            return False
        return self.used >= cap

    # Back-compat spelling for external readers; ``used`` is the field.
    @property
    def tick_count(self) -> int:
        return self.used
