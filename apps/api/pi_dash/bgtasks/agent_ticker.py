# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-issue agent ticking — scanner and per-ticker worker tasks.

The scanner (``scan_due_tickers``) runs once a minute under Celery Beat
and fans out one ``fire_tick`` task per due ticker row. ``fire_tick``
performs the atomic claim under ``select_for_update`` and dispatches the
continuation run.

Distinct from ``pi_dash.bgtasks.scheduler`` which fires user-authored
project-level schedulers — this module is the per-issue continuation
clock and is system-armed on state transitions, not user-installed.

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

from pi_dash.db.models.issue_agent_ticker import (
    INFINITE_MAX_TICKS,
    IssueAgentTicker,
)

logger = logging.getLogger("pi_dash.worker")


@shared_task(name="pi_dash.bgtasks.agent_ticker.scan_due_tickers")
def scan_due_tickers() -> int:
    """Fan out ``fire_tick`` tasks for every due ticker row.

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
        IssueAgentTicker.objects.filter(
            enabled=True,
            next_run_at__lte=now,
        )
        .annotate(_cap=effective_cap)
        .filter(Q(_cap=INFINITE_MAX_TICKS) | Q(tick_count__lt=F("_cap")))
        .order_by("next_run_at")
        .values_list("id", flat=True)
    )
    for ticker_id in due_ids:
        fire_tick.delay(str(ticker_id))
    if due_ids:
        logger.info("agent_ticker.scan: dispatched %d fire_tick tasks", len(due_ids))
    return len(due_ids)


@shared_task(name="pi_dash.bgtasks.agent_ticker.fire_tick")
def fire_tick(ticker_id: str) -> bool:
    """Per-ticker worker. Atomically claims and dispatches.

    Returns ``True`` if a continuation run was dispatched, ``False`` if the
    fire was skipped (race lost, ticker changed, no active In Progress
    state, run already in flight, etc.).
    """
    from pi_dash.orchestration.scheduling import (
        DELEGATION_STATE_NAME,
        TRIGGER_TICK,
        dispatch_continuation_run,
    )

    with transaction.atomic():
        ticker = (
            IssueAgentTicker.objects.select_for_update(of=("self",))
            .select_related("issue", "issue__state", "issue__project")
            .filter(pk=ticker_id)
            .first()
        )
        if ticker is None:
            return False

        # Re-check after acquiring the lock — Comment & Run, another tick
        # firing on the same row, or a disarm could have moved things.
        if not ticker.enabled:
            return False
        now = timezone.now()
        if ticker.next_run_at is None or ticker.next_run_at > now:
            return False

        cap = ticker.effective_max_ticks()
        if cap != INFINITE_MAX_TICKS and ticker.tick_count >= cap:
            # Already at cap — disarm and bail.
            ticker.enabled = False
            ticker.save(update_fields=["enabled", "updated_at"])
            return False

        issue = ticker.issue
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
                "agent_ticker.fire_tick: skip issue=%s reason=active-run-exists",
                issue.pk,
            )
            return False
        if orchestration_service._latest_prior_run(issue) is None:
            logger.info(
                "agent_ticker.fire_tick: skip issue=%s reason=no-prior-run",
                issue.pk,
            )
            return False

        # Claim: advance the clock first, then dispatch. We capture the
        # pre-claim values so we can roll back below if dispatch returns
        # None for a reason the pre-claim skips didn't catch (no-pod,
        # no-creator, race-inserted active run between the pre-check and
        # dispatch's own re-check). Without rollback the tick budget is
        # silently burned and the ticker may auto-disarm at cap on a
        # tick that never produced a run.
        prev_tick_count = ticker.tick_count
        prev_next_run_at = ticker.next_run_at
        prev_enabled = ticker.enabled

        ticker.tick_count = ticker.tick_count + 1
        ticker.last_tick_at = now
        from pi_dash.db.models.issue_agent_ticker import jitter_seconds
        from datetime import timedelta

        interval = ticker.effective_interval_seconds()
        ticker.next_run_at = now + timedelta(seconds=interval + jitter_seconds(interval))

        cap_hit_now = (
            cap != INFINITE_MAX_TICKS and ticker.tick_count >= cap
        )
        if cap_hit_now:
            # Disarm immediately (no more fires); the In Progress → Paused
            # transition is deferred to the run-terminate hook (§4.4.1).
            ticker.enabled = False

        ticker.save(
            update_fields=[
                "tick_count",
                "last_tick_at",
                "next_run_at",
                "enabled",
                "updated_at",
            ]
        )

    # Dispatch outside the transaction so the ticker write is visible to
    # other tx-bounded readers and so drain_pod_by_id (which is scheduled
    # via transaction.on_commit inside _create_continuation_run) actually
    # fires.
    run = dispatch_continuation_run(issue, triggered_by=TRIGGER_TICK)
    if run is None:
        # Dispatch failed post-claim — restore the ticker so the budget
        # isn't wasted and any cap-disarm we just applied is undone.
        # last_tick_at intentionally NOT rolled back: it's an observability
        # field for "we attempted a tick at this time," not a budget input.
        with transaction.atomic():
            rollback = (
                IssueAgentTicker.objects.select_for_update()
                .filter(pk=ticker_id)
                .first()
            )
            if rollback is not None:
                rollback.tick_count = prev_tick_count
                rollback.next_run_at = prev_next_run_at
                rollback.enabled = prev_enabled
                rollback.save(
                    update_fields=[
                        "tick_count",
                        "next_run_at",
                        "enabled",
                        "updated_at",
                    ]
                )
        logger.info(
            "agent_ticker.fire_tick: dispatch returned None issue=%s; rolled back claim",
            issue.pk,
        )
        return False
    logger.info(
        "agent_ticker.fire_tick: dispatched run=%s issue=%s tick_count=%d cap_hit=%s",
        run.pk,
        issue.pk,
        ticker.tick_count,
        cap_hit_now,
    )
    return True
