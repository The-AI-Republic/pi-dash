# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Celery tasks for runner lifecycle maintenance.

Registered via ``apps/api/apple_pi_dash/celery.py`` and the existing
``INSTALLED_APPS`` celery beat schedule. These tasks are idempotent.
"""

from __future__ import annotations

import logging
from datetime import timedelta

from celery import shared_task
from django.db import transaction
from django.utils import timezone

from apple_pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    ApprovalRequest,
    ApprovalStatus,
    Runner,
    RunnerStatus,
)
from apple_pi_dash.runner.services.pubsub import send_to_runner

logger = logging.getLogger(__name__)

HEARTBEAT_OFFLINE_GRACE = timedelta(seconds=90)


@shared_task(name="runner.expire_stale_approvals")
def expire_stale_approvals() -> int:
    """Move expired ApprovalRequests to EXPIRED and cancel their runs.

    Returns the number of approvals expired.
    """
    now = timezone.now()
    expired = 0
    qs = ApprovalRequest.objects.select_related("agent_run__runner").filter(
        status=ApprovalStatus.PENDING,
        expires_at__lt=now,
    )
    for approval in qs.iterator():
        with transaction.atomic():
            ApprovalRequest.objects.filter(pk=approval.pk).update(
                status=ApprovalStatus.EXPIRED,
                decided_at=now,
            )
            run = approval.agent_run
            if run.status in {
                AgentRunStatus.AWAITING_APPROVAL,
                AgentRunStatus.RUNNING,
            }:
                AgentRun.objects.filter(pk=run.pk).update(
                    status=AgentRunStatus.CANCELLED,
                    ended_at=now,
                )
                if run.runner_id:
                    send_to_runner(
                        run.runner_id,
                        {
                            "v": 1,
                            "type": "cancel",
                            "run_id": str(run.id),
                            "reason": "approval_timeout",
                        },
                    )
        expired += 1
    if expired:
        logger.info("expired %s stale approval(s)", expired)
    return expired


@shared_task(name="runner.mark_offline_runners")
def mark_offline_runners() -> int:
    """Mark ONLINE runners as OFFLINE when their heartbeat is stale.

    This complements the channels consumer's ``disconnect`` path for the case
    where a process dies without a clean teardown.
    """
    threshold = timezone.now() - HEARTBEAT_OFFLINE_GRACE
    affected = (
        Runner.objects.filter(
            status=RunnerStatus.ONLINE,
        )
        .exclude(last_heartbeat_at__gte=threshold)
        .update(status=RunnerStatus.OFFLINE)
    )
    if affected:
        logger.info("marked %s runner(s) offline via heartbeat timeout", affected)
    return affected
