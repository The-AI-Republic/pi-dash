# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Scanner — reconcile creates targets, fan-out queues only eligible, ineligible
cursors get advanced, kill switch short-circuits."""

from __future__ import annotations


import pytest
from django.utils import timezone

from pi_dash.bgtasks import loop as loop_tasks
from pi_dash.db.models import LoopTarget, SkipReason
from pi_dash.tests.contract.assistant.conftest import configure_llm
from pi_dash.tests.contract.loop.conftest import make_job, make_target

pytestmark = pytest.mark.django_db


def test_kill_switch_short_circuits(world, settings):
    settings.LOOP_ENABLED = False
    assert loop_tasks.scan_due_targets() == 0


def test_reconcile_creates_targets_for_active_edges(world, settings, mocker):
    # Force reconcile to run regardless of wall-clock minute.
    settings.LOOP_RECONCILE_EVERY_MINUTES = 1
    mocker.patch.object(loop_tasks, "fire_loop_target")
    job = make_job(min_role=5)  # guest-eligible so every edge qualifies
    now = timezone.now().replace(second=0, microsecond=0)
    created = loop_tasks._reconcile_targets(now)
    # world has 4 active members in ws + 1 in other_ws = 5 edges.
    assert created == LoopTarget.objects.filter(job=job).count()
    assert created >= 5
    # New targets are scheduled for the FUTURE (no immediate burst).
    assert LoopTarget.objects.filter(job=job, next_run_at__lte=now).count() == 0


def test_reconcile_is_idempotent(world, settings, mocker):
    settings.LOOP_RECONCILE_EVERY_MINUTES = 1
    make_job(min_role=5)
    now = timezone.now().replace(second=0, microsecond=0)
    first = loop_tasks._reconcile_targets(now)
    second = loop_tasks._reconcile_targets(now)
    assert first >= 5
    assert second == 0  # nothing new the second time


def test_fanout_queues_only_eligible_due(world, settings, fernet_key, mocker):
    settings.LOOP_RECONCILE_EVERY_MINUTES = 99  # disable reconcile this tick
    fire = mocker.patch.object(loop_tasks, "fire_loop_target")
    job = make_job(min_role=15)
    configure_llm(world.member)
    eligible = make_target(job, world.ws, world.member)
    # guest has no LLM config and is below min_role → ineligible, must be advanced not queued.
    ineligible = make_target(job, world.ws, world.guest)

    n = loop_tasks.scan_due_targets()
    assert n == 1
    fire.delay.assert_called_once_with(str(eligible.id))

    ineligible.refresh_from_db()
    assert ineligible.last_skip_reason in {SkipReason.MIN_ROLE, SkipReason.LLM_CONFIG_MISSING}
    assert ineligible.next_run_at > timezone.now()  # cursor advanced past now


def test_fanout_respects_dispatch_cap(world, settings, fernet_key, mocker):
    settings.LOOP_RECONCILE_EVERY_MINUTES = 99
    settings.LOOP_MAX_DISPATCH_PER_TICK = 1
    fire = mocker.patch.object(loop_tasks, "fire_loop_target")
    job = make_job(min_role=15)
    for u in (world.member, world.outsider):
        configure_llm(u)
        make_target(job, world.ws, u)
    n = loop_tasks.scan_due_targets()
    assert n == 1
    assert fire.delay.call_count == 1
