# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Project Scheduler — Beat scanner and per-binding fire.

The scanner (:func:`scan_due_bindings`) runs once a minute under Celery
Beat and fans out one :func:`fire_scheduler_binding` task per due binding.
``fire_scheduler_binding`` follows the three-phase pattern documented in
``.ai_design/project_scheduler/design.md`` §6.2:

1. **Claim** under SFU: re-check enabled, skip when ``last_run`` is
   non-terminal, advance ``next_run_at`` from the cron expression, commit.
2. **Dispatch** outside the SFU transaction. The dispatcher registers
   ``transaction.on_commit(drain_pod_by_id)`` and holding the row lock
   across that callback breaks pod drain.
3. **Rollback** post-dispatch only when dispatch returned ``None``:
   re-acquire SFU and restore the pre-claim ``next_run_at`` so the budget
   isn't burned silently.

**Beat must run as a singleton** — multiple Beat schedulers double the
scan rate. The atomic claim under SFU keeps fan-out race-safe regardless,
but the scan rate is still doubled.
"""

from __future__ import annotations

import logging
from datetime import datetime, timedelta, timezone as dt_timezone
from typing import Optional

from celery import shared_task
from croniter import croniter, CroniterBadCronError
from django.conf import settings
from django.db import transaction
from django.db.models import Q
from django.utils import timezone

from pi_dash.db.models.scheduler import SchedulerBinding
from pi_dash.runner.models import AgentRunStatus

logger = logging.getLogger("pi_dash.worker")


NON_TERMINAL_STATUSES = frozenset(
    {
        AgentRunStatus.QUEUED,
        AgentRunStatus.ASSIGNED,
        AgentRunStatus.RUNNING,
        AgentRunStatus.AWAITING_APPROVAL,
        AgentRunStatus.AWAITING_REAUTH,
        AgentRunStatus.PAUSED_AWAITING_INPUT,
        AgentRunStatus.BLOCKED,
    }
)


def _is_enabled() -> bool:
    """Instance-level kill switch — see design §10 Rollout."""
    return getattr(settings, "SCHEDULER_ENABLED", True)


def _next_fire_from_cron(cron_expr: str, *, now: Optional[datetime] = None) -> Optional[datetime]:
    """Return the next datetime ``cron_expr`` is due after ``now`` (UTC).

    Returns ``None`` if ``cron_expr`` is malformed — callers treat that as
    a configuration error and skip.
    """
    base = now or timezone.now()
    if base.tzinfo is None:
        base = base.replace(tzinfo=dt_timezone.utc)
    try:
        itr = croniter(cron_expr, base)
        nxt = itr.get_next(datetime)
        if nxt.tzinfo is None:
            nxt = nxt.replace(tzinfo=dt_timezone.utc)
        return nxt
    except (CroniterBadCronError, ValueError) as e:
        logger.warning("scheduler.cron_parse: bad cron=%r err=%s", cron_expr, e)
        return None


def _is_last_run_in_flight(binding: SchedulerBinding) -> bool:
    """True when the previous run is still non-terminal.

    A binding whose previous run is queued / running / awaiting-anything /
    blocked is treated as "still in flight" — the new tick is skipped per
    the design's concurrency policy (§9.1).
    """
    if binding.last_run_id is None:
        return False
    last_run = binding.last_run
    return last_run.status in NON_TERMINAL_STATUSES


@shared_task(name="pi_dash.bgtasks.scheduler.scan_due_bindings")
def scan_due_bindings() -> int:
    """Fan out ``fire_scheduler_binding`` tasks for every due binding.

    Returns the number of fan-outs (mostly for logging / tests).
    """
    if not _is_enabled():
        return 0

    now = timezone.now()
    # NULL next_run_at means "never fired; due immediately." Postgres NULL
    # semantics exclude rows with NULL from `__lte=`, so the OR clause is
    # required. Also filter out bindings whose scheduler has been
    # soft-deleted: the SoftDeleteModel cascade is async (see db/mixins.py
    # `soft_delete_related_objects.delay`), so there's a real window where
    # a binding still has deleted_at IS NULL but its parent doesn't.
    due_ids = list(
        SchedulerBinding.objects.filter(
            enabled=True,
            scheduler__is_enabled=True,
            scheduler__deleted_at__isnull=True,
        )
        .filter(Q(next_run_at__lte=now) | Q(next_run_at__isnull=True))
        .order_by("next_run_at")
        .values_list("id", flat=True)
    )
    for binding_id in due_ids:
        fire_scheduler_binding.delay(str(binding_id))
    if due_ids:
        logger.info(
            "scheduler.scan: dispatched %d fire_scheduler_binding tasks",
            len(due_ids),
        )
    return len(due_ids)


@shared_task(
    name="pi_dash.bgtasks.scheduler.fire_scheduler_binding",
    bind=True,
    max_retries=0,
)
def fire_scheduler_binding(self, binding_id: str) -> bool:
    """Fire one binding through the three-phase claim/dispatch/rollback.

    Returns ``True`` if a run was dispatched, ``False`` if skipped.
    """
    if not _is_enabled():
        return False

    # ----- Phase 1: Claim under SFU and advance next_run_at -----
    prev_next_run_at: Optional[datetime] = None
    prompt: str = ""
    binding_workspace_id = None
    binding_pk = None

    with transaction.atomic():
        binding = (
            SchedulerBinding.objects.select_for_update(of=("self",))
            .select_related("scheduler", "last_run")
            .filter(pk=binding_id, deleted_at__isnull=True)
            .first()
        )
        if binding is None:
            return False
        if not binding.enabled:
            return False
        if not binding.scheduler.is_enabled:
            return False
        now = timezone.now()
        # If next_run_at is in the future, this firing is racing the
        # scanner; skip without advancing.
        if binding.next_run_at is not None and binding.next_run_at > now:
            return False
        if _is_last_run_in_flight(binding):
            logger.info(
                "scheduler.fire: skip binding=%s reason=last-run-in-flight status=%s",
                binding.pk,
                binding.last_run.status,
            )
            return False

        nxt = _next_fire_from_cron(binding.cron, now=now)
        if nxt is None:
            # Bad cron — record the error and disable the binding so we
            # don't re-attempt every minute. Also clear next_run_at so
            # that if the user later re-enables (without changing cron),
            # the API path through ProjectSchedulerBindingDetailEndpoint
            # is the only way back in — and that path validates cron.
            binding.last_error = f"invalid cron expression: {binding.cron!r}"[:1000]
            binding.enabled = False
            binding.next_run_at = None
            binding.save(
                update_fields=["last_error", "enabled", "next_run_at", "updated_at"]
            )
            return False

        prev_next_run_at = binding.next_run_at
        binding_workspace_id = binding.workspace_id
        binding_pk = binding.pk
        binding.next_run_at = nxt
        # Clear previous short-circuit error; a real terminate-hook update
        # will rewrite this if dispatch succeeds and the run later fails.
        if binding.last_error:
            binding.last_error = ""
        binding.save(update_fields=["next_run_at", "last_error", "updated_at"])

        # Resolve prompt while we still have the row in scope.
        base_prompt = binding.scheduler.prompt or ""
        if binding.extra_context:
            prompt = f"{base_prompt}\n\n{binding.extra_context}".strip()
        else:
            prompt = base_prompt.strip()

    # ----- Phase 2: Dispatch outside the transaction -----
    from pi_dash.orchestration.service import dispatch_scheduler_run

    # Re-fetch the binding without SFU; the dispatcher only needs the FK
    # values it copies onto the AgentRun.
    binding_for_dispatch = (
        SchedulerBinding.objects.select_related("scheduler")
        .filter(pk=binding_pk)
        .first()
    )
    if binding_for_dispatch is None:
        # Deleted between phases — nothing to roll back to.
        return False

    run = dispatch_scheduler_run(binding_for_dispatch, prompt)

    # ----- Phase 3a: Success — record the run pointer -----
    if run is not None:
        with transaction.atomic():
            success = (
                SchedulerBinding.objects.select_for_update(of=("self",))
                .filter(pk=binding_pk)
                .first()
            )
            if success is not None:
                success.last_run = run
                success.last_error = ""
                # Use save() so auto_now on updated_at fires; queryset
                # .update() bypasses it (`auto_now` is set in pre_save).
                success.save(update_fields=["last_run", "last_error", "updated_at"])
        logger.info(
            "scheduler.fire: dispatched run=%s binding=%s",
            run.pk,
            binding_pk,
        )
        return True

    # ----- Phase 3b: Failure — rollback the next_run_at advance -----
    with transaction.atomic():
        rollback_target = (
            SchedulerBinding.objects.select_for_update(of=("self",))
            .filter(pk=binding_pk)
            .first()
        )
        if rollback_target is not None:
            rollback_target.next_run_at = prev_next_run_at
            rollback_target.last_error = "dispatch failed (no default pod or no creator)"[:1000]
            rollback_target.save(
                update_fields=["next_run_at", "last_error", "updated_at"]
            )
    logger.info(
        "scheduler.fire: dispatch returned None binding=%s; rolled back next_run_at",
        binding_pk,
    )
    return False
