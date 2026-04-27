# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Periodic agent ticking — scanner and per-schedule worker tasks.

The scanner (``scan_due_schedules``) runs once a minute under Celery Beat
and fans out one ``fire_tick`` task per due schedule row. ``fire_tick``
performs the atomic claim under ``select_for_update`` and dispatches the
continuation run.

See ``.ai_design/issue_ticking_system/design.md`` §6 for the design and
§11 (item 11) for the atomic-claim invariant.

**Beat must run as a singleton** — multiple Beat schedulers double the
scan rate. This is standard Celery deployment hygiene; the actual claim
in ``fire_tick`` is still race-safe regardless.
"""

from __future__ import annotations

import logging

from celery import shared_task
from django.db import transaction
from django.db.models import F, Q
from django.db.models.functions import Coalesce
from django.utils import timezone

from pi_dash.db.models.issue_agent_schedule import (
    INFINITE_MAX_TICKS,
    IssueAgentSchedule,
)

logger = logging.getLogger("pi_dash.worker")


@shared_task(name="pi_dash.bgtasks.agent_schedule.scan_due_schedules")
def scan_due_schedules() -> int:
    """Fan out ``fire_tick`` tasks for every due schedule row.

    The actual claim happens inside ``fire_tick`` under ``select_for_update``.
    Returns the number of fan-outs (mostly for logging / tests).
    """
    now = timezone.now()
    # Effective cap = override if set, else project default. We compute it
    # via Coalesce so the scanner can filter under-cap rows at the DB level
    # (instead of fanning out tasks that fire_tick will then have to disarm
    # on the backstop). ``-1`` means infinite — admit unconditionally.
    effective_cap = Coalesce(
        F("max_ticks"), F("issue__project__agent_default_max_ticks")
    )
    due_ids = list(
        IssueAgentSchedule.objects.filter(
            enabled=True,
            next_run_at__lte=now,
        )
        .annotate(_cap=effective_cap)
        .filter(Q(_cap=INFINITE_MAX_TICKS) | Q(tick_count__lt=F("_cap")))
        .order_by("next_run_at")
        .values_list("id", flat=True)
    )
    for sched_id in due_ids:
        fire_tick.delay(str(sched_id))
    if due_ids:
        logger.info("agent_schedule.scan: dispatched %d fire_tick tasks", len(due_ids))
    return len(due_ids)


@shared_task(name="pi_dash.bgtasks.agent_schedule.fire_tick")
def fire_tick(sched_id: str) -> bool:
    """Per-schedule worker. Atomically claims and dispatches.

    Returns ``True`` if a continuation run was dispatched, ``False`` if the
    fire was skipped (race lost, schedule changed, no active In Progress
    state, run already in flight, etc.).
    """
    from pi_dash.orchestration.scheduling import (
        DELEGATION_STATE_NAME,
        TRIGGER_TICK,
        dispatch_continuation_run,
    )

    with transaction.atomic():
        sched = (
            IssueAgentSchedule.objects.select_for_update(of=("self",))
            .select_related("issue", "issue__state", "issue__project")
            .filter(pk=sched_id)
            .first()
        )
        if sched is None:
            return False

        # Re-check after acquiring the lock — Comment & Run, another tick
        # firing on the same row, or a disarm could have moved things.
        if not sched.enabled:
            return False
        now = timezone.now()
        if sched.next_run_at is None or sched.next_run_at > now:
            return False

        cap = sched.effective_max_ticks()
        if cap != INFINITE_MAX_TICKS and sched.tick_count >= cap:
            # Already at cap — disarm and bail.
            sched.enabled = False
            sched.save(update_fields=["enabled", "updated_at"])
            return False

        issue = sched.issue
        # Mirror ``_is_delegation_trigger``: only the literally-named
        # "In Progress" state can tick in v1.
        if issue.state is None or issue.state.name != DELEGATION_STATE_NAME:
            return False

        # Pre-claim skips. All three reasons (active run, no prior run,
        # no pod) leave next_run_at unchanged so the scanner re-checks
        # next minute. Budget is only consumed when we actually have a
        # run we can dispatch — otherwise a misconfigured issue would
        # burn through 24 ticks doing nothing and auto-pause itself.
        from pi_dash.orchestration import service as orchestration_service

        if orchestration_service._active_run_for(issue) is not None:
            logger.info(
                "agent_schedule.fire_tick: skip issue=%s reason=active-run-exists",
                issue.pk,
            )
            return False
        if orchestration_service._latest_prior_run(issue) is None:
            logger.info(
                "agent_schedule.fire_tick: skip issue=%s reason=no-prior-run",
                issue.pk,
            )
            return False

        # Claim: advance the clock first, then dispatch.
        sched.tick_count = sched.tick_count + 1
        sched.last_tick_at = now
        from pi_dash.db.models.issue_agent_schedule import jitter_seconds
        from datetime import timedelta

        interval = sched.effective_interval_seconds()
        sched.next_run_at = now + timedelta(seconds=interval + jitter_seconds(interval))

        cap_hit_now = (
            cap != INFINITE_MAX_TICKS and sched.tick_count >= cap
        )
        if cap_hit_now:
            # Disarm immediately (no more fires); the In Progress → Paused
            # transition is deferred to the run-terminate hook (§4.4.1).
            sched.enabled = False

        sched.save(
            update_fields=[
                "tick_count",
                "last_tick_at",
                "next_run_at",
                "enabled",
                "updated_at",
            ]
        )

    # Dispatch outside the transaction so the schedule write is visible
    # to other tx-bounded readers and so drain_pod_by_id (which is
    # scheduled via transaction.on_commit inside _create_continuation_run)
    # actually fires.
    run = dispatch_continuation_run(issue, triggered_by=TRIGGER_TICK)
    if run is None:
        logger.info(
            "agent_schedule.fire_tick: dispatch returned None issue=%s",
            issue.pk,
        )
        return False
    logger.info(
        "agent_schedule.fire_tick: dispatched run=%s issue=%s tick_count=%d cap_hit=%s",
        run.pk,
        issue.pk,
        sched.tick_count,
        cap_hit_now,
    )
    return True
