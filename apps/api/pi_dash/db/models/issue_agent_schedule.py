# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-issue agent ticking schedule.

See ``.ai_design/issue_ticking_system/design.md`` §7.1.
"""

from __future__ import annotations

import random
from typing import Optional

from django.db import models

from .base import BaseModel
from .issue import Issue


DEFAULT_INTERVAL_SECONDS = 10800  # 3 hours
DEFAULT_MAX_TICKS = 24            # 3 days at 3h cadence
INFINITE_MAX_TICKS = -1
JITTER_FRACTION = 0.1


def jitter_seconds(interval_seconds: int) -> float:
    """Uniform random offset in ``[0, interval × JITTER_FRACTION)``.

    Spreads out scheduled fires so that bulk transitions (e.g. sprint planning
    moving 50 issues to In Progress at once) do not re-cluster every cycle.
    """
    if interval_seconds <= 0:
        return 0.0
    return random.uniform(0, interval_seconds * JITTER_FRACTION)


class IssueAgentSchedule(BaseModel):
    """The clock that drives periodic agent re-invocation for one issue.

    Exactly one row per issue (``issue`` is unique). Arming, disarming, and
    user-edited overrides all mutate this row in place rather than creating
    additional rows.
    """

    issue = models.OneToOneField(
        Issue,
        on_delete=models.CASCADE,
        related_name="agent_schedule",
    )

    # User-configured overrides. ``null`` means "inherit from project".
    interval_seconds = models.IntegerField(null=True, blank=True)
    max_ticks = models.IntegerField(null=True, blank=True)
    user_disabled = models.BooleanField(default=False)

    # Runtime state.
    next_run_at = models.DateTimeField(null=True, blank=True)
    tick_count = models.IntegerField(default=0)
    last_tick_at = models.DateTimeField(null=True, blank=True)
    enabled = models.BooleanField(default=True)

    class Meta:
        db_table = "issue_agent_schedule"
        verbose_name = "Issue Agent Schedule"
        verbose_name_plural = "Issue Agent Schedules"
        indexes = [
            models.Index(
                fields=["enabled", "next_run_at"],
                name="iasched_enabled_next_run_idx",
            ),
        ]

    def __str__(self) -> str:
        return f"IssueAgentSchedule(issue={self.issue_id}, enabled={self.enabled})"

    # ------------------------------------------------------------------
    # Effective values (override-or-project-default)
    # ------------------------------------------------------------------

    def effective_interval_seconds(self) -> int:
        """Return the interval to use, falling back to the project default."""
        if self.interval_seconds is not None and self.interval_seconds > 0:
            return self.interval_seconds
        project = self.issue.project
        return getattr(project, "agent_default_interval_seconds", DEFAULT_INTERVAL_SECONDS)

    def effective_max_ticks(self) -> int:
        """Return the cap to use. ``-1`` means infinite."""
        if self.max_ticks is not None:
            return self.max_ticks
        project = self.issue.project
        return getattr(project, "agent_default_max_ticks", DEFAULT_MAX_TICKS)

    def cap_reached(self) -> bool:
        """Has this schedule already exhausted its tick budget?"""
        cap = self.effective_max_ticks()
        if cap == INFINITE_MAX_TICKS:
            return False
        return self.tick_count >= cap
