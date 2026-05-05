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
from django.db import models, transaction
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
from pi_dash.runner.services import outbox, run_lifecycle
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


# ---- Per-active-run agent stall watchdog ---------------------------------
#
# Cloud-side backstop for the runner's own internal stall watchdog —
# see ``.ai_design/runner_agent_bridge/design.md`` §4.5.3. Fires only on
# runs where the snapshot's ``observed_run_id`` matches the run id (so we
# never reap a run from a previous-run snapshot), the snapshot row is
# *fresh* (so we don't reap when the runner stops reporting altogether —
# heartbeat-staleness is handled by ``mark_offline_runners`` / etc.), and
# the agent itself has been silent past the threshold.

@shared_task(name="runner.reconcile_stalled_runs")
def reconcile_stalled_runs() -> int:
    """Mark BUSY runs FAILED when the agent subprocess has gone silent.

    Three conjuncts must all hold for a run to be reaped:

    1. ``RunnerLiveState.observed_run_id == AgentRun.id`` — the snapshot
       describes *this* run, not a previous one whose row hasn't been
       overwritten yet.
    2. ``RunnerLiveState.updated_at`` is within
       ``RUNNER_AGENT_OBSERVABILITY_STALE_SECS`` — the runner is still
       reporting (we're not reacting to a stale row from a downgraded
       runner).
    3. ``RunnerLiveState.last_event_at`` is older than
       ``RUNNER_AGENT_STALL_THRESHOLD_SECS`` — the agent itself has gone
       silent.

    Runs in ``AWAITING_APPROVAL`` / ``AWAITING_REAUTH`` are excluded so
    operator-driven pauses are never failed by the watchdog.
    Pre-observability runners pass cleanly: their ``last_event_at`` is
    NULL and ``__lt`` excludes NULL.
    """
    threshold = int(getattr(settings, "RUNNER_AGENT_STALL_THRESHOLD_SECS", 360))
    snapshot_freshness = int(
        getattr(settings, "RUNNER_AGENT_OBSERVABILITY_STALE_SECS", 90)
    )
    now = timezone.now()
    cutoff = now - timedelta(seconds=threshold)
    snapshot_cutoff = now - timedelta(seconds=snapshot_freshness)

    # AWAITING_APPROVAL / AWAITING_REAUTH are intentionally excluded
    # from the active-run set: they're operator-driven pauses, not
    # silence on the agent's part. The runner's snapshot during those
    # states will look "stalled" by construction; failing the run from
    # this watchdog would race the user. Filter directly on the two
    # active states instead of subtracting from BUSY_STATUSES so a
    # future status added to BUSY_STATUSES doesn't silently sneak past
    # the exclusion.
    active_statuses = (AgentRunStatus.ASSIGNED, AgentRunStatus.RUNNING)
    stalled = (
        AgentRun.objects.filter(status__in=active_statuses)
        .filter(
            runner__live_state__observed_run_id=models.F("id"),
            runner__live_state__updated_at__gte=snapshot_cutoff,
            runner__live_state__last_event_at__lt=cutoff,
        )
        .select_related("runner")
    )

    reaped = 0
    for run in stalled:
        if run.runner is None:
            continue
        # Per-row guard: a single bad finalize (DB constraint, optimistic-
        # concurrency conflict, scheduler-hook error) must not abort the
        # whole sweep — the next 30s tick would then re-walk the same poison
        # row and never reap the rest. Log + continue.
        try:
            run_lifecycle.finalize_run_terminal(
                run.runner,
                run.id,
                AgentRunStatus.FAILED,
                error_detail=f"agent stalled: no events for >{threshold}s",
            )
            reaped += 1
        except Exception:
            logger.exception(
                "reconcile_stalled_runs: failed to reap run %s",
                run.id,
            )
    if reaped:
        logger.info(
            "reconcile_stalled_runs reaped %s stalled run(s) (threshold=%ss)",
            reaped,
            threshold,
        )
    return reaped
