# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Debounced state-transition dispatch worker.

A state change that targets a ticking state schedules one of these tasks
via ``apply_async(countdown=DEBOUNCE_SECONDS)`` (see
``orchestration.service._schedule_pending_dispatch``). When it fires it
re-reads the issue under a row lock, validates the debounce token, and
dispatches against the issue's *current* state — coalescing any rapid
sequence of transitions into a single dispatch on the final state.

A further transition inside the window bumps the token, so a stale task is
a harmless no-op.

See ``.ai_design/state_transition_debounce/design.md``.
"""

from __future__ import annotations

import logging

from celery import shared_task

logger = logging.getLogger("pi_dash.worker")


@shared_task(name="pi_dash.bgtasks.state_transition_debounce.dispatch_debounced_transition")
def dispatch_debounced_transition(issue_id: str, token: int) -> str:
    """Fire the debounced dispatch for ``issue_id`` if ``token`` is current.

    Returns the :class:`~pi_dash.orchestration.service.TransitionOutcome`
    reason string (for logging / tests).
    """
    from pi_dash.orchestration.service import run_debounced_dispatch

    outcome = run_debounced_dispatch(issue_id, token)
    logger.info(
        "state_transition_debounce: issue=%s token=%s reason=%s",
        issue_id,
        token,
        outcome.reason,
    )
    return outcome.reason
