# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-issue agent ticker.

The continuation clock that re-invokes the agent on a single in-progress
issue. Internal lifecycle machinery, system-armed on Issue state
transitions; it is not a user-authored periodic task. Renamed from
``IssueAgentSchedule`` to free the word "scheduler" for a future
user-authored project-level scheduling concept.

See ``.ai_design/issue_ticking_system/design.md`` §7.1.
"""

from __future__ import annotations

import random

from django.db import models

from .base import BaseModel
from .issue import Issue


DEFAULT_INTERVAL_SECONDS = 10800  # 3 hours
DEFAULT_MAX_TICKS = 24            # 3 days at 3h cadence
INFINITE_MAX_TICKS = -1
JITTER_FRACTION = 0.1


class TickerDisarmReason(models.TextChoices):
    """Why the ticker is currently disarmed.

    ``maybe_apply_deferred_pause`` only auto-Pauses the issue when
    ``disarm_reason == CAP_HIT``. Terminal-signal disarms
    (``completed``/``blocked``) leave the issue in place for the
    human to act. See ``.ai_design/create_review_state/design.md``
    §4.5 / §7.3.
    """

    NONE = "", "None"
    LEFT_TICKING_STATE = "left_ticking_state", "Left Ticking State"
    CAP_HIT = "cap_hit", "Cap Hit"
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

    Exactly one row per issue (``issue`` is unique). Arming, disarming, and
    user-edited overrides all mutate this row in place rather than creating
    additional rows.
    """

    issue = models.OneToOneField(
        Issue,
        on_delete=models.CASCADE,
        related_name="agent_ticker",
    )

    # User-configured overrides for the **In Progress** phase.
    # ``null`` means "inherit from project default".
    interval_seconds = models.IntegerField(null=True, blank=True)
    max_ticks = models.IntegerField(null=True, blank=True)
    # User-configured overrides for the **In Review** phase. Mirror
    # the In Progress pair; ``null`` means "inherit from project's
    # ``agent_review_default_*``". See
    # ``.ai_design/create_review_state/design.md`` §6.3.
    review_interval_seconds = models.IntegerField(null=True, blank=True)
    review_max_ticks = models.IntegerField(null=True, blank=True)
    # User-configured overrides for the **In Test** phase. In Test is a
    # sibling of In Review, not a variant of it — it owns its own pair
    # so a cap grant in one phase can never inflate the other's budget.
    # See ``.ai_design/create_test_state/design.md`` §3.2.
    test_interval_seconds = models.IntegerField(null=True, blank=True)
    test_max_ticks = models.IntegerField(null=True, blank=True)
    user_disabled = models.BooleanField(default=False)

    # Runtime state.
    next_run_at = models.DateTimeField(null=True, blank=True)
    tick_count = models.IntegerField(default=0)
    last_tick_at = models.DateTimeField(null=True, blank=True)
    enabled = models.BooleanField(default=True)
    # Why the ticker is currently disarmed. Empty string when armed.
    # See TickerDisarmReason for semantics; load-bearing for the
    # cap-hit-only auto-pause gate in ``maybe_apply_deferred_pause``.
    disarm_reason = models.CharField(
        max_length=32,
        blank=True,
        default="",
        choices=TickerDisarmReason.choices,
    )
    # On entering Review from a different ticking phase, the latest
    # implementation-phase run is captured here so the reverse
    # transition (Review → In Progress) can resume that exact session
    # rather than parenting off the latest review run.
    # See design §6.3.
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
    # Effective values (override-or-project-default)
    # ------------------------------------------------------------------

    def _cadence_fields(self):
        """Return the :class:`CadenceFields` for the issue's current phase.

        Every ticking phase owns an independent override/default column
        pair, so this resolver never has to know which phase it is
        looking at — it just asks the registry. A state that is not a
        registered ticking state (including a custom workspace state
        inside a ticking group) falls back to the implementation pair.
        """
        # Local import keeps the model file free of orchestration
        # imports at module load time (orchestration imports state).
        from pi_dash.orchestration.agent_phases import cadence_fields_for

        return cadence_fields_for(self.issue.state)

    def effective_interval_seconds(self) -> int:
        """Return the interval to use for the issue's current phase.

        Falls back through: per-issue override for that phase → the
        project default for that phase → the registry constant.
        """
        fields = self._cadence_fields()
        override = getattr(self, fields.ticker_interval, None)
        if override is not None and override > 0:
            return override
        return getattr(
            self.issue.project,
            fields.project_interval,
            fields.default_interval,
        )

    def effective_max_ticks(self) -> int:
        """Return the cap to use for the issue's current phase. ``-1``
        means infinite. Same resolution chain as
        ``effective_interval_seconds`` against the max-ticks column.
        """
        fields = self._cadence_fields()
        override = getattr(self, fields.ticker_max_ticks, None)
        if override is not None:
            return override
        return getattr(
            self.issue.project,
            fields.project_max_ticks,
            fields.default_max_ticks,
        )

    def cap_reached(self) -> bool:
        """Has this ticker already exhausted its tick budget?"""
        cap = self.effective_max_ticks()
        if cap == INFINITE_MAX_TICKS:
            return False
        return self.tick_count >= cap
