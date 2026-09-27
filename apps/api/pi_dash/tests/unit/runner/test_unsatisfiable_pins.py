# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Matcher hardening for unsatisfiable pins + BUSY-but-dead runner sweep.

PDASHOSS01-233 (follow-up to PDASHOSS01-231): a QUEUED run pinned to a
runner that cannot take work — wedged BUSY with nothing in flight, offline,
or silently dead — used to wait until a human cleared the pin by hand (the
release-pin escape hatch). Three reconciliation points close that:

1. ``reconcile_unsatisfiable_pins`` releases a wedged-busy pinned runner
   (fresh heartbeat, no busy-status run) back to ONLINE, keeping the pin so
   the runner serves its own continuation on the next drain.
2. The same pass clears pins held past ``RUNNER_PIN_AUTO_RELEASE_SECS``
   while the pinned runner is neither assignable nor legitimately working,
   so any eligible runner can serve the run.
3. The offline sweepers (``mark_offline_runners`` / ``sweep_stale_runners``)
   now also flip BUSY runners with a stale heartbeat and no busy-status run
   to OFFLINE — a daemon that dies silently no longer parks its row in BUSY.
"""

from __future__ import annotations

from datetime import timedelta
from unittest.mock import patch

import pytest
from django.utils import timezone

from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerProvisioning,
    RunnerStatus,
)
from pi_dash.runner.services.matcher import reconcile_unsatisfiable_pins
from pi_dash.runner.tasks import (
    mark_offline_runners,
    reconcile_unsatisfiable_pins as reconcile_unsatisfiable_pins_task,
    sweep_stale_runners,
)

STALE = timedelta(minutes=10)
PAST_BOUND = timedelta(seconds=601)


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture(autouse=True)
def _run_on_commit_immediately():
    """Drains schedule their WS sends via on_commit; tests run outside an
    atomic block so the callbacks would otherwise never fire."""
    with patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()):
        yield


@pytest.fixture(autouse=True)
def _silence_ws():
    with patch("pi_dash.runner.services.pubsub.send_to_runner"):
        yield


def _make_runner(user, workspace, pod, name, *, status=RunnerStatus.BUSY, heartbeat_age=timedelta(0), **extra):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        status=status,
        last_heartbeat_at=timezone.now() - heartbeat_age,
        refresh_token_generation=1,
        enrolled_at=timezone.now(),
        **extra,
    )


def _make_pinned_run(user, workspace, pod, pinned_runner, *, age=timedelta(0), **extra):
    run = AgentRun.objects.create(
        workspace=workspace,
        owner=user,
        created_by=user,
        pod=pod,
        pinned_runner=pinned_runner,
        status=AgentRunStatus.QUEUED,
        prompt="continuation",
        **extra,
    )
    if age:
        # created_at is auto_now_add; backdate through the queryset.
        AgentRun.objects.filter(pk=run.pk).update(created_at=timezone.now() - age)
        run.refresh_from_db()
    return run


def _make_active_run(user, workspace, pod, runner, *, status=AgentRunStatus.RUNNING):
    return AgentRun.objects.create(
        workspace=workspace,
        owner=user,
        created_by=user,
        pod=pod,
        runner=runner,
        status=status,
        prompt="test",
        assigned_at=timezone.now(),
        started_at=timezone.now(),
    )


# ---------------------------------------------------------------------------
# (1) Wedged-busy pinned runner: released, pin preserved
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_wedged_busy_pin_released_and_served(db, create_user, workspace, pod):
    """The validation-sketch case (a): a queued run pinned to a busy runner
    with no non-terminal run ends up assigned — to that same runner, whose
    stale BUSY the reconcile clears."""
    runner = _make_runner(create_user, workspace, pod, "wedged")
    run = _make_pinned_run(create_user, workspace, pod, runner)

    reconcile_unsatisfiable_pins_task()

    runner.refresh_from_db()
    run.refresh_from_db()
    assert runner.status == RunnerStatus.ONLINE
    assert run.status == AgentRunStatus.ASSIGNED
    assert run.runner_id == runner.id
    assert run.pinned_runner_id == runner.id


@pytest.mark.unit
def test_wedged_busy_dead_runner_not_resurrected(db, create_user, workspace, pod):
    """A wedged runner that also stopped heartbeating must not be flipped
    ONLINE by the pin reconcile — the BUSY-but-dead sweep owns it and sends
    it OFFLINE instead."""
    runner = _make_runner(create_user, workspace, pod, "dead-wedged", heartbeat_age=STALE)
    run = _make_pinned_run(create_user, workspace, pod, runner)

    reconcile_unsatisfiable_pins_task()

    runner.refresh_from_db()
    run.refresh_from_db()
    assert runner.status == RunnerStatus.BUSY
    assert run.status == AgentRunStatus.QUEUED
    assert run.pinned_runner_id == runner.id


@pytest.mark.unit
def test_busy_runner_with_active_run_not_released(db, create_user, workspace, pod):
    """A pinned runner legitimately serving another run is not a wedge; the
    pin waits for the normal finalization release."""
    runner = _make_runner(create_user, workspace, pod, "working")
    _make_active_run(create_user, workspace, pod, runner)
    run = _make_pinned_run(create_user, workspace, pod, runner)

    reconcile_unsatisfiable_pins_task()

    runner.refresh_from_db()
    run.refresh_from_db()
    assert runner.status == RunnerStatus.BUSY
    assert run.status == AgentRunStatus.QUEUED
    assert run.pinned_runner_id == runner.id


# ---------------------------------------------------------------------------
# (2) Bounded pin auto-release
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_pin_past_bound_to_offline_runner_released_and_assigned(db, create_user, workspace, pod):
    """The validation-sketch case (b): a run pinned past the auto-release
    bound to an offline runner has its pin cleared and is assigned to
    another eligible runner in the same pass."""
    dead = _make_runner(create_user, workspace, pod, "gone", status=RunnerStatus.OFFLINE, heartbeat_age=STALE)
    healthy = _make_runner(create_user, workspace, pod, "healthy", status=RunnerStatus.ONLINE)
    run = _make_pinned_run(create_user, workspace, pod, dead, age=PAST_BOUND)

    reconcile_unsatisfiable_pins_task()

    run.refresh_from_db()
    assert run.pinned_runner_id is None
    assert run.status == AgentRunStatus.ASSIGNED
    assert run.runner_id == healthy.id


@pytest.mark.unit
def test_pin_within_bound_kept(db, create_user, workspace, pod):
    dead = _make_runner(create_user, workspace, pod, "gone", status=RunnerStatus.OFFLINE, heartbeat_age=STALE)
    _make_runner(create_user, workspace, pod, "healthy", status=RunnerStatus.ONLINE)
    run = _make_pinned_run(create_user, workspace, pod, dead, age=timedelta(seconds=30))

    reconcile_unsatisfiable_pins_task()

    run.refresh_from_db()
    assert run.pinned_runner_id == dead.id
    assert run.status == AgentRunStatus.QUEUED


@pytest.mark.unit
def test_pin_to_assignable_runner_kept_past_bound(db, create_user, workspace, pod):
    """An ONLINE, heartbeat-fresh pinned runner can serve the pin — age
    alone must never break the affinity. Nothing to reconcile, so the pass
    is a no-op; the ordinary event-driven drain serves this pin."""
    mine = _make_runner(create_user, workspace, pod, "mine", status=RunnerStatus.ONLINE)
    run = _make_pinned_run(create_user, workspace, pod, mine, age=PAST_BOUND)

    assert reconcile_unsatisfiable_pins() == set()

    run.refresh_from_db()
    assert run.pinned_runner_id == mine.id
    assert run.status == AgentRunStatus.QUEUED


@pytest.mark.unit
def test_pin_to_alive_working_runner_kept_past_bound(db, create_user, workspace, pod):
    """A BUSY runner with a fresh heartbeat and a real busy-status run is
    mid-flight, not wedged: the pin outlives the bound and waits for the
    finalization release, even while another runner idles."""
    working = _make_runner(create_user, workspace, pod, "working")
    _make_active_run(create_user, workspace, pod, working)
    _make_runner(create_user, workspace, pod, "idle", status=RunnerStatus.ONLINE)
    run = _make_pinned_run(create_user, workspace, pod, working, age=PAST_BOUND)

    reconcile_unsatisfiable_pins_task()

    run.refresh_from_db()
    assert run.pinned_runner_id == working.id
    assert run.status == AgentRunStatus.QUEUED


@pytest.mark.unit
def test_managed_runner_pin_never_cleared(db, create_user, workspace, pod):
    """A managed (desktop-bundled) run without its pin is unservable by
    construction, so the auto-release must never touch it."""
    desktop = _make_runner(
        create_user,
        workspace,
        pod,
        "desktop",
        status=RunnerStatus.OFFLINE,
        heartbeat_age=STALE,
        provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
    )
    run = _make_pinned_run(
        create_user,
        workspace,
        pod,
        desktop,
        age=PAST_BOUND,
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
    )

    reconcile_unsatisfiable_pins_task()

    run.refresh_from_db()
    assert run.pinned_runner_id == desktop.id
    assert run.status == AgentRunStatus.QUEUED


@pytest.mark.unit
def test_bound_zero_disables_auto_release(db, create_user, workspace, pod, settings):
    settings.RUNNER_PIN_AUTO_RELEASE_SECS = 0
    dead = _make_runner(create_user, workspace, pod, "gone", status=RunnerStatus.OFFLINE, heartbeat_age=STALE)
    run = _make_pinned_run(create_user, workspace, pod, dead, age=PAST_BOUND)

    assert reconcile_unsatisfiable_pins() == set()

    run.refresh_from_db()
    assert run.pinned_runner_id == dead.id


# ---------------------------------------------------------------------------
# (3) BUSY-but-dead runner sweep
# ---------------------------------------------------------------------------


@pytest.mark.unit
@pytest.mark.parametrize("sweeper", [mark_offline_runners, sweep_stale_runners])
def test_sweep_flips_dead_busy_runner_offline(db, create_user, workspace, pod, sweeper):
    """The validation-sketch sweeper case: BUSY, stale heartbeat, no
    non-terminal run — reconciled to OFFLINE."""
    runner = _make_runner(create_user, workspace, pod, "dead-busy", heartbeat_age=STALE)
    _make_active_run(create_user, workspace, pod, runner, status=AgentRunStatus.FAILED)

    assert sweeper() == 1

    runner.refresh_from_db()
    assert runner.status == RunnerStatus.OFFLINE


@pytest.mark.unit
@pytest.mark.parametrize("sweeper", [mark_offline_runners, sweep_stale_runners])
def test_sweep_keeps_dead_busy_runner_with_active_run(db, create_user, workspace, pod, sweeper):
    """While a busy-status run remains, the stall reaper owns the wind-down;
    its finalization releases the runner and the next sweep catches the row
    through the ONLINE branch."""
    runner = _make_runner(create_user, workspace, pod, "reaping", heartbeat_age=STALE)
    _make_active_run(create_user, workspace, pod, runner)

    assert sweeper() == 0

    runner.refresh_from_db()
    assert runner.status == RunnerStatus.BUSY


@pytest.mark.unit
@pytest.mark.parametrize("sweeper", [mark_offline_runners, sweep_stale_runners])
def test_sweep_keeps_alive_busy_runner(db, create_user, workspace, pod, sweeper):
    """A heartbeating BUSY runner is never the sweeper's business, wedged or
    not — the finalization/session-open release handles a live wedge."""
    runner = _make_runner(create_user, workspace, pod, "alive-busy")

    assert sweeper() == 0

    runner.refresh_from_db()
    assert runner.status == RunnerStatus.BUSY


# ---------------------------------------------------------------------------
# End to end: the incident shape
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_dead_busy_runner_with_old_pin_recovers_end_to_end(db, create_user, workspace, pod):
    """A daemon dies silently mid-wedge: sweep flips the runner OFFLINE,
    the pin reconcile then clears the over-bound pin and the drain assigns
    the run to a healthy runner — no human escape hatch involved."""
    dead = _make_runner(create_user, workspace, pod, "dead", heartbeat_age=STALE)
    healthy = _make_runner(create_user, workspace, pod, "healthy", status=RunnerStatus.ONLINE)
    run = _make_pinned_run(create_user, workspace, pod, dead, age=PAST_BOUND)

    mark_offline_runners()
    reconcile_unsatisfiable_pins_task()

    dead.refresh_from_db()
    run.refresh_from_db()
    assert dead.status == RunnerStatus.OFFLINE
    assert run.pinned_runner_id is None
    assert run.status == AgentRunStatus.ASSIGNED
    assert run.runner_id == healthy.id
