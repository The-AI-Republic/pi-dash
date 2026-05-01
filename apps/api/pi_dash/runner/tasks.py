# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Celery tasks for runner lifecycle maintenance.

Registered via ``apps/api/pi_dash/celery.py`` and the existing
``INSTALLED_APPS`` celery beat schedule. These tasks are idempotent.
"""

from __future__ import annotations

import logging
from datetime import timedelta

from celery import shared_task
from django.conf import settings
from django.db import transaction
from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    ApprovalRequest,
    ApprovalStatus,
    RunMessageDedupe,
    Runner,
    RunnerSession,
    RunnerStatus,
)
from pi_dash.runner.services import outbox
from pi_dash.runner.services.pubsub import send_to_runner

logger = logging.getLogger(__name__)

HEARTBEAT_OFFLINE_GRACE = timedelta(seconds=90)


@shared_task(name="runner.expire_stale_approvals")
def expire_stale_approvals() -> int:
    """Move expired ApprovalRequests to EXPIRED and cancel their runs."""
    now = timezone.now()
    expired = 0
    pending_ids = list(
        ApprovalRequest.objects.filter(
            status=ApprovalStatus.PENDING, expires_at__lt=now
        ).values_list("pk", flat=True)
    )
    for approval_id in pending_ids:
        runner_id = None
        run_id = None
        with transaction.atomic():
            approval = (
                ApprovalRequest.objects.select_for_update()
                .select_related("agent_run")
                .filter(pk=approval_id, status=ApprovalStatus.PENDING)
                .first()
            )
            if approval is None:
                continue
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
                runner_id = run.runner_id
                run_id = run.id
        if runner_id:
            send_to_runner(
                runner_id,
                {
                    "type": "cancel",
                    "run_id": str(run_id),
                    "reason": "approval_timeout",
                },
            )
        expired += 1
    if expired:
        logger.info("expired %s stale approval(s)", expired)
    return expired


@shared_task(name="runner.mark_offline_runners")
def mark_offline_runners() -> int:
    """Heartbeat-staleness offline detection."""
    threshold = timezone.now() - HEARTBEAT_OFFLINE_GRACE
    affected = (
        Runner.objects.filter(status=RunnerStatus.ONLINE)
        .exclude(last_heartbeat_at__gte=threshold)
        .update(status=RunnerStatus.OFFLINE)
    )
    if affected:
        logger.info("marked %s runner(s) offline via heartbeat timeout", affected)
    return affected


# ---- Per-runner HTTPS transport sweepers ---------------------------------


@shared_task(name="runner.sweep_idle_sessions")
def sweep_idle_sessions() -> int:
    """Mark long-idle ``RunnerSession`` rows revoked.

    See ``design.md`` §7.10. Sessions with no poll activity for
    ``2 * long_poll_interval_secs`` are evicted with reason
    ``idle_timeout``; pub/sub eviction signal fires per row.
    """
    poll_secs = int(getattr(settings, "LONG_POLL_INTERVAL_SECS", 25))
    threshold = timezone.now() - timedelta(seconds=poll_secs * 2)
    sessions = list(
        RunnerSession.objects.filter(
            revoked_at__isnull=True, last_seen_at__lt=threshold
        ).values_list("id", "runner_id")
    )
    if not sessions:
        return 0
    sids = [sid for sid, _ in sessions]
    RunnerSession.objects.filter(id__in=sids).update(
        revoked_at=timezone.now(), revoked_reason="idle_timeout"
    )
    for sid, runner_id in sessions:
        outbox.clear_session_marker(sid)
        outbox.publish_session_eviction(
            runner_id, old_session_id=str(sid), new_session_id=""
        )
    logger.info("sweep_idle_sessions evicted %s session(s)", len(sessions))
    return len(sessions)


@shared_task(name="runner.sweep_stale_runners")
def sweep_stale_runners() -> int:
    """Flip ONLINE runners to OFFLINE when last_heartbeat_at is too old."""
    threshold = timezone.now() - timedelta(
        seconds=int(getattr(settings, "RUNNER_OFFLINE_THRESHOLD_SECS", 50))
    )
    affected = (
        Runner.objects.filter(status=RunnerStatus.ONLINE)
        .exclude(last_heartbeat_at__gte=threshold)
        .update(status=RunnerStatus.OFFLINE)
    )
    if affected:
        logger.info("sweep_stale_runners flipped %s offline", affected)
    return affected


@shared_task(name="runner.sweep_old_streams")
def sweep_old_streams() -> int:
    """Sweeper-driven trim + cleanup for runner streams.

    Three jobs (``design.md`` §7.10):

    1. Old-consumer reaping for revoked sessions.
    2. PEL+undelivered-aware trim per active runner stream.
    3. Orphaned-stream deletion for revoked / long-idle runners.
    """
    cutoff = int(getattr(settings, "RUNNER_STREAM_MIN_RETENTION_SECS", 3600))
    time_cutoff_id = outbox.id_for_secs_ago(cutoff)
    trimmed_count = 0

    active_runner_ids = list(
        RunnerSession.objects.filter(revoked_at__isnull=True).values_list(
            "runner_id", flat=True
        )
    )
    for rid in set(active_runner_ids):
        try:
            removed = outbox.safe_trim_runner_stream(
                rid, time_cutoff_id=time_cutoff_id
            )
            if removed:
                trimmed_count += removed
        except Exception:
            logger.exception("safe_trim_runner_stream failed for %s", rid)

    # Orphaned-stream deletion for runners flagged for cleanup.
    for rid in outbox.due_runners_for_stream_cleanup():
        try:
            outbox.delete_runner_stream(rid)
            outbox.remove_stream_cleanup_marker(rid)
        except Exception:
            logger.exception("delete_runner_stream failed for %s", rid)
    return trimmed_count


@shared_task(name="runner.sweep_run_message_dedupe")
def sweep_run_message_dedupe() -> int:
    """Delete idempotency rows older than the configured TTL."""
    ttl = int(getattr(settings, "RUN_MESSAGE_DEDUPE_TTL_SECS", 604800))
    cutoff = timezone.now() - timedelta(seconds=ttl)
    deleted, _ = RunMessageDedupe.objects.filter(created_at__lt=cutoff).delete()
    if deleted:
        logger.info("sweep_run_message_dedupe deleted %s row(s)", deleted)
    return deleted
