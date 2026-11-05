# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Periodic maintenance for strictly pinned runs waiting on their runner.

An automatic run on an issue assigned to a runner that is offline is created
``QUEUED`` with ``error_code = desktop_not_connected`` ("Waiting for
<machine>") and delivered the moment that runner comes back
(``matcher.drain_for_runner``). That is the right behaviour for a laptop
closed over lunch, and the wrong one for a machine that never comes back — so
this sweep gives the wait a bound.

Selected by ``AgentRun.pin_strict`` rather than by executor kind
(``.ai_design/built_engine_as_runner/design.md`` §9): a strict pin never falls
back to the pod, so a run strictly pinned to an offline own-agent runner
expires too instead of waiting for ever. Legacy ``managed_runner`` queued runs
were given ``pin_strict`` by migration ``runner.0035``.
"""

from __future__ import annotations

import logging
from datetime import timedelta

from celery import shared_task
from django.conf import settings
from django.utils import timezone

from pi_dash.runner.models import AgentRun, AgentRunStatus
from pi_dash.runner.services.agent_run_finalization import finalize_agent_run
from pi_dash.runner.services.dispatch_rules import WAITING_FOR_RUNNER

logger = logging.getLogger(__name__)


def _expiry_message(run: AgentRun) -> str:
    runner = run.pinned_runner
    machine = runner.machine_display_name if runner is not None else "The assigned runner"
    hours = max(1, round(settings.MANAGED_RUNNER_QUEUED_MAX_AGE_SECS / 3600))
    return (
        f"{machine} is offline. This run waited {hours} h for it and was not started; "
        "run the issue again once it is back, or assign the issue to another runner or its pod."
    )


@shared_task(name="managed_runner.expire_waiting_runs")
def expire_waiting_runs() -> int:
    """Fail strict runs that waited past ``MANAGED_RUNNER_QUEUED_MAX_AGE_SECS``.

    Only ``QUEUED`` rows are eligible: a run the runner already accepted has
    left this state, and one the user cancelled is terminal. Assigned-but-stuck
    runs are the existing heartbeat reaper's job, not this one.
    """
    cutoff = timezone.now() - timedelta(seconds=settings.MANAGED_RUNNER_QUEUED_MAX_AGE_SECS)
    stale = list(
        AgentRun.objects.filter(
            pin_strict=True,
            status=AgentRunStatus.QUEUED,
            created_at__lt=cutoff,
        )
        .select_related("pinned_runner__dev_machine")
        .order_by("created_at")[:500]
    )

    expired = 0
    for run in stale:
        if finalize_agent_run(
            run.id,
            AgentRunStatus.FAILED,
            expected_status=AgentRunStatus.QUEUED,
            updates={"error_code": WAITING_FOR_RUNNER, "error": _expiry_message(run)},
        ):
            expired += 1
            logger.info("managed_runner.queued_expired run=%s", run.id)
    return expired
