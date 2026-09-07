# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Periodic maintenance for managed runs waiting on a closed desktop.

An automatic run whose creator's desktop is offline is created ``QUEUED`` with
``error_code = desktop_not_connected`` and delivered the moment that machine
heartbeats (``matcher.drain_for_runner``). That is the right behaviour for a
laptop closed over lunch, and the wrong one for a laptop that never comes back
— so this sweep gives the wait a bound.
"""

from __future__ import annotations

import logging
from datetime import timedelta

from celery import shared_task
from django.conf import settings
from django.utils import timezone

from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.managed_runner.errors import ManagedRunnerReason
from pi_dash.runner.models import AgentRun, AgentRunStatus
from pi_dash.runner.services.agent_run_finalization import finalize_agent_run

logger = logging.getLogger(__name__)


@shared_task(name="managed_runner.expire_waiting_runs")
def expire_waiting_runs() -> int:
    """Fail managed runs that waited past ``MANAGED_RUNNER_QUEUED_MAX_AGE_SECS``.

    Only ``QUEUED`` rows are eligible: a run the desktop already accepted has
    left this state, and one the user cancelled is terminal. Assigned-but-stuck
    runs are the existing heartbeat reaper's job, not this one.
    """
    cutoff = timezone.now() - timedelta(seconds=settings.MANAGED_RUNNER_QUEUED_MAX_AGE_SECS)
    stale = AgentRun.objects.filter(
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
        status=AgentRunStatus.QUEUED,
        created_at__lt=cutoff,
    ).values_list("id", flat=True)

    expired = 0
    for run_id in list(stale[:500]):
        if finalize_agent_run(
            run_id,
            AgentRunStatus.FAILED,
            updates={
                "error_code": ManagedRunnerReason.NOT_CONNECTED,
                "error": (
                    "Pi Dash Agent never came online for this run. Open the desktop app and run the issue again."
                ),
            },
        ):
            expired += 1
            logger.info("managed_runner.queued_expired run=%s", run_id)
    return expired
