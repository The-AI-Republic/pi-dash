# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Loop (Auto Project Management) — Beat scanner and per-target fire.

The scanner (:func:`scan_due_targets`) runs once a minute under Celery Beat and
does two things: reconcile (create missing targets for new membership edges,
throttled) and fan out (queue :func:`fire_loop_target` for every eligible due
target). ``fire_loop_target`` claims one target under SFU, re-checks eligibility
freshest-wins, advances the cursor, and hands off to
:func:`pi_dash.loop.dispatch.dispatch_loop_turn`.

Unlike the scheduler there is **no rollback phase**: dispatch here is local row
creation, not a remote pod match that can transiently fail. See
``.ai_design/loop_project_management/design.md`` §7.1–7.4.

**Beat must run as a singleton** — multiple Beat schedulers double the scan
rate. The atomic SFU claim keeps fan-out race-safe regardless.
"""

from __future__ import annotations

import logging
from datetime import timedelta
from zlib import crc32

from celery import shared_task
from django.conf import settings
from django.db.models import Exists, OuterRef
from django.utils import timezone

from pi_dash.bgtasks._rrule import next_fire_from_rrule
from pi_dash.db.models import LoopJob, LoopTarget, WorkspaceMember
from pi_dash.loop import dispatch, eligibility

logger = logging.getLogger("pi_dash.worker")


def _is_enabled() -> bool:
    return getattr(settings, "LOOP_ENABLED", True)


def _stagger(job_id, workspace_id, user_id) -> timedelta:
    """Deterministic per-edge offset within the stagger window (no randomness in
    scheduling paths → reproducible tests, stable per edge)."""
    window = max(1, int(getattr(settings, "LOOP_STAGGER_WINDOW_MINUTES", 60)))
    seed = f"{job_id}:{workspace_id}:{user_id}".encode()
    return timedelta(minutes=crc32(seed) % window)


def _next_fire_for_job(job: LoopJob, *, now=None):
    return next_fire_from_rrule(
        dtstart=job.dtstart, rrule_str=job.rrule or "", tzid=job.tzid or "UTC", now=now
    )


# --------------------------------------------------------------------------- #
# Scanner
# --------------------------------------------------------------------------- #

def _reconcile_targets(now) -> int:
    """Create missing ``LoopTarget`` rows for (enabled job × active edge).

    Throttled to once per ``LOOP_RECONCILE_EVERY_MINUTES`` — a new member waits
    at most that long for a cursor, and their first fire is next occurrence +
    stagger regardless. Returns the number of targets created.
    """
    every = max(1, int(getattr(settings, "LOOP_RECONCILE_EVERY_MINUTES", 15)))
    if now.minute % every != 0:
        return 0

    created = 0
    for job in LoopJob.objects.filter(enabled=True, deleted_at__isnull=True):
        nxt = _next_fire_for_job(job, now=now)
        if nxt is None:
            # Bad RRULE — admin API validation should make this unreachable.
            logger.warning("loop.reconcile: job=%s has unusable rrule=%r", job.slug, job.rrule)
            continue
        has_target = LoopTarget.objects.filter(
            job=job,
            deleted_at__isnull=True,
            workspace_id=OuterRef("workspace_id"),
            user_id=OuterRef("member_id"),
        )
        edges = (
            WorkspaceMember.objects.filter(
                is_active=True, member__is_active=True, deleted_at__isnull=True
            )
            .annotate(_has=Exists(has_target))
            .filter(_has=False)
            .values_list("workspace_id", "member_id")
        )
        batch = [
            LoopTarget(
                job=job,
                workspace_id=ws,
                user_id=uid,
                next_run_at=nxt + _stagger(job.id, ws, uid),
            )
            for ws, uid in edges.iterator(chunk_size=1000)
        ]
        if batch:
            LoopTarget.objects.bulk_create(batch, ignore_conflicts=True, batch_size=500)
            created += len(batch)
    if created:
        logger.info("loop.reconcile: created %d targets", created)
    return created


def _advance_ineligible_due(now) -> int:
    """Advance the cursor for due-but-ineligible targets so they aren't
    re-examined every minute. One bulk pass, cursor recomputed once per job.
    Returns the number advanced.
    """
    eligible_ids = set(eligibility.eligible_due_targets(now).values_list("id", flat=True))
    due = (
        eligibility.due_targets(now)
        .select_related("job")
        .exclude(id__in=eligible_ids)
    )
    # Compute each job's next fire once.
    next_by_job: dict = {}
    to_update = []
    for target in due.iterator(chunk_size=1000):
        job = target.job
        if job.id not in next_by_job:
            next_by_job[job.id] = _next_fire_for_job(job, now=now)
        nxt = next_by_job[job.id]
        reason = eligibility.check(target)
        target.next_run_at = (
            nxt + _stagger(job.id, target.workspace_id, target.user_id) if nxt else None
        )
        target.last_skipped_at = now
        target.last_skip_reason = reason or ""
        to_update.append(target)
    if to_update:
        LoopTarget.objects.bulk_update(
            to_update,
            ["next_run_at", "last_skipped_at", "last_skip_reason", "updated_at"],
            batch_size=500,
        )
    return len(to_update)


@shared_task(name="pi_dash.bgtasks.loop.scan_due_targets")
def scan_due_targets() -> int:
    """Reconcile, then fan out ``fire_loop_target`` for eligible due targets.

    Returns the number of fan-outs (for logging / tests).
    """
    if not _is_enabled():
        return 0

    now = timezone.now()
    _reconcile_targets(now)

    cap = max(1, int(getattr(settings, "LOOP_MAX_DISPATCH_PER_TICK", 100)))
    ids = list(
        eligibility.eligible_due_targets(now)
        .order_by("next_run_at")
        .values_list("id", flat=True)[:cap]
    )
    for target_id in ids:
        fire_loop_target.delay(str(target_id))

    # Targets that are due but ineligible still need their cursor advanced, or
    # they'd be re-scanned every minute forever. (Over-cap *eligible* targets
    # are intentionally left due — backpressure; they drain next tick.)
    _advance_ineligible_due(now)

    if ids:
        logger.info("loop.scan: dispatched %d fire_loop_target tasks", len(ids))
    return len(ids)


# --------------------------------------------------------------------------- #
# Per-target fire
# --------------------------------------------------------------------------- #

@shared_task(name="pi_dash.bgtasks.loop.fire_loop_target", bind=True, max_retries=0)
def fire_loop_target(self, target_id: str) -> bool:
    """Claim one target under SFU, re-check eligibility, advance the cursor, and
    dispatch a turn. Returns ``True`` if a turn was queued.
    """
    if not _is_enabled():
        return False

    from django.db import transaction

    # ----- Phase 1: claim, re-check eligibility, advance cursor -----
    with transaction.atomic():
        target = (
            LoopTarget.objects.select_for_update(of=("self",))
            .select_related("job", "workspace", "user")
            .filter(pk=target_id, deleted_at__isnull=True)
            .first()
        )
        if target is None:
            return False
        now = timezone.now()
        # Future cursor = raced the scanner; another fire already claimed.
        if target.next_run_at is not None and target.next_run_at > now:
            return False

        job = target.job
        nxt = _next_fire_for_job(job, now=now)
        target.next_run_at = (
            nxt + _stagger(job.id, target.workspace_id, target.user_id) if nxt else None
        )

        skip = eligibility.check(target)
        if skip is not None:
            target.last_skipped_at = now
            target.last_skip_reason = skip
            target.save(
                update_fields=["next_run_at", "last_skipped_at", "last_skip_reason", "updated_at"]
            )
            return False
        target.save(update_fields=["next_run_at", "updated_at"])

    # ----- Phase 2: dispatch a turn (own transaction; no rollback) -----
    return dispatch.dispatch_loop_turn(str(target_id))
