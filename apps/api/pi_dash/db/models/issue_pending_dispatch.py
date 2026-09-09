# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-issue pending (debounced) state-transition dispatch intent.

A state change that targets a ticking state no longer dispatches an agent
run inline. Instead the transition records its intent here and schedules a
delayed job that fires after a short quiet period (see
``orchestration.service.DEBOUNCE_SECONDS``). A further state change within
the window replaces the intent and bumps ``token``; the earlier delayed job
becomes a no-op because its token no longer matches the row.

Exactly one row per issue (``issue`` is unique). Rapid transitions mutate
this row in place rather than creating additional rows.

See ``.ai_design/state_transition_debounce/design.md``.
"""

from __future__ import annotations

from django.db import models

from .base import BaseModel
from .issue import Issue
from .state import State


class IssuePendingDispatch(BaseModel):
    """The debounce record for one issue's pending dispatch."""

    issue = models.OneToOneField(
        Issue,
        on_delete=models.CASCADE,
        related_name="pending_dispatch",
    )

    #: Monotonically increasing per-issue token. Every state transition
    #: bumps it; a delayed dispatch job carries the token it was scheduled
    #: with and no-ops when the persisted token has moved on. Also bumped
    #: (without scheduling a job) when a transition lands on a non-ticking
    #: state, which cancels any pending dispatch.
    token = models.BigIntegerField(default=0)

    #: When the currently-armed delayed job should fire. ``None`` means no
    #: dispatch is pending (the last transition cancelled it). Advisory —
    #: the ``token`` match is authoritative.
    dispatch_at = models.DateTimeField(null=True, blank=True)

    #: The transition's source/target states, captured so the delayed job
    #: can resolve the cross-phase session shape (fresh-session vs
    #: resume-parent) exactly as an inline transition would. ``to_state``
    #: is advisory; the job re-reads the issue's current state as
    #: authoritative. Both use ``SET_NULL`` so state deletion can't cascade
    #: a debounce row away mid-window.
    from_state = models.ForeignKey(
        State,
        null=True,
        blank=True,
        on_delete=models.SET_NULL,
        related_name="+",
    )
    to_state = models.ForeignKey(
        State,
        null=True,
        blank=True,
        on_delete=models.SET_NULL,
        related_name="+",
    )

    class Meta:
        db_table = "issue_pending_dispatch"
        verbose_name = "Issue Pending Dispatch"
        verbose_name_plural = "Issue Pending Dispatches"

    def __str__(self) -> str:
        return f"IssuePendingDispatch(issue={self.issue_id}, token={self.token})"
