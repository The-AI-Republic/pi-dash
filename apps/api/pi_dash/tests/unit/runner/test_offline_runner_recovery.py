# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Recovery of runs stranded on a runner that stopped heartbeating.

Covers ``_pinned_runner_for`` (no pin to an unavailable runner) and the
``release_pins_on_offline_runners`` / ``fail_runs_on_offline_runners``
sweeps in ``pi_dash.runner.tasks``.
"""

from __future__ import annotations

from datetime import timedelta
from unittest.mock import patch

import pytest
from django.test import override_settings
from django.utils import timezone

from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.db.models import Issue, State
from pi_dash.orchestration.service import _active_run_for, _pinned_runner_for
from pi_dash.runner import tasks
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services import session_service


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture(autouse=True)
def _stub_send_to_runner():
    with patch("pi_dash.runner.services.pubsub.send_to_runner") as mock:
        yield mock


@pytest.fixture(autouse=True)
def _stub_terminal_effects():
    """Terminal hooks publish to Celery; these tests only assert run rows."""
    with patch("pi_dash.runner.services.agent_run_finalization._publish_effects"):
        yield


@pytest.fixture(autouse=True)
def _run_on_commit_immediately():
    with patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()):
        yield


@pytest.fixture
def issue(workspace, project, create_user):
    state = State.objects.create(name="In Progress", project=project, group="started")
    return Issue.objects.create(
        name="Stranded", workspace=workspace, project=project, state=state, created_by=create_user
    )


def _make_runner(user, workspace, pod, name, *, silent_for_s=1, status=RunnerStatus.ONLINE):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        status=status,
        last_heartbeat_at=(None if silent_for_s is None else timezone.now() - timedelta(seconds=silent_for_s)),
    )


def _make_run(user, workspace, pod, *, status=AgentRunStatus.QUEUED, age_s=0, **fields):
    run = AgentRun.objects.create(
        owner=user,
        created_by=user,
        workspace=workspace,
        pod=pod,
        prompt="x",
        status=status,
        **fields,
    )
    then = timezone.now() - timedelta(seconds=age_s)
    stamps = {"created_at": then}
    if status != AgentRunStatus.QUEUED:
        stamps["assigned_at"] = then
    AgentRun.objects.filter(pk=run.pk).update(**stamps)
    run.refresh_from_db()
    return run


# ---------------- _pinned_runner_for ----------------


@pytest.mark.unit
def test_pin_kept_for_online_runner_with_fresh_heartbeat(db, create_user, workspace, pod):
    runner = _make_runner(create_user, workspace, pod, "alive")
    parent = _make_run(create_user, workspace, pod, status=AgentRunStatus.COMPLETED, runner=runner)

    assert _pinned_runner_for(parent, pod) == runner


@pytest.mark.unit
def test_pin_kept_for_busy_runner(db, create_user, workspace, pod):
    # A daemon reporting busy (another run, or a chat turn) is alive: the
    # follow-up waits for it rather than losing repo locality.
    runner = _make_runner(create_user, workspace, pod, "busy", status=RunnerStatus.BUSY)
    parent = _make_run(create_user, workspace, pod, status=AgentRunStatus.COMPLETED, runner=runner)

    assert _pinned_runner_for(parent, pod) == runner


@pytest.mark.unit
@pytest.mark.parametrize(
    "status,silent_for_s",
    [
        (RunnerStatus.OFFLINE, 3600),
        (RunnerStatus.REVOKED, 1),
        # Still flagged ONLINE (the offline sweep has not run yet) but silent.
        (RunnerStatus.ONLINE, 3600),
        (RunnerStatus.ONLINE, None),
        # ``mark_offline_runners`` never flips BUSY, so a dead runner can
        # keep this status forever; the heartbeat is what rules it out.
        (RunnerStatus.BUSY, 3600),
    ],
)
def test_no_pin_to_unavailable_runner(db, create_user, workspace, pod, status, silent_for_s):
    runner = _make_runner(create_user, workspace, pod, "gone", status=status, silent_for_s=silent_for_s)
    parent = _make_run(create_user, workspace, pod, status=AgentRunStatus.FAILED, runner=runner)

    assert _pinned_runner_for(parent, pod) is None


# ---------------- release_pins_on_offline_runners ----------------


@pytest.mark.unit
def test_release_pin_hands_run_to_another_runner(db, create_user, workspace, pod):
    gone = _make_runner(create_user, workspace, pod, "gone", status=RunnerStatus.OFFLINE, silent_for_s=3600)
    alive = _make_runner(create_user, workspace, pod, "alive")
    run = _make_run(create_user, workspace, pod, pinned_runner=gone, age_s=3600)

    assert tasks.release_pins_on_offline_runners() == 1

    run.refresh_from_db()
    assert run.pinned_runner_id is None
    assert run.status == AgentRunStatus.ASSIGNED
    assert run.runner_id == alive.id


@pytest.mark.unit
def test_release_pin_for_runner_that_never_heartbeated(db, create_user, workspace, pod):
    never = _make_runner(create_user, workspace, pod, "never", status=RunnerStatus.OFFLINE, silent_for_s=None)
    run = _make_run(create_user, workspace, pod, pinned_runner=never, age_s=3600)

    assert tasks.release_pins_on_offline_runners() == 1

    run.refresh_from_db()
    assert run.pinned_runner_id is None


@pytest.mark.unit
def test_release_pin_spares_run_younger_than_the_grace(db, create_user, workspace, pod):
    gone = _make_runner(create_user, workspace, pod, "gone", status=RunnerStatus.OFFLINE, silent_for_s=3600)
    run = _make_run(create_user, workspace, pod, pinned_runner=gone, age_s=60)

    assert tasks.release_pins_on_offline_runners() == 0

    run.refresh_from_db()
    assert run.pinned_runner_id == gone.id


@pytest.mark.unit
def test_release_pin_clears_parent_thread_id(db, create_user, workspace, pod):
    gone = _make_runner(create_user, workspace, pod, "gone", status=RunnerStatus.OFFLINE, silent_for_s=3600)
    parent = _make_run(create_user, workspace, pod, status=AgentRunStatus.FAILED, runner=gone, thread_id="thread-1")
    run = _make_run(create_user, workspace, pod, pinned_runner=gone, parent_run=parent, age_s=3600)

    assert tasks.release_pins_on_offline_runners() == 1

    run.refresh_from_db()
    parent.refresh_from_db()
    assert run.pinned_runner_id is None
    assert run.status == AgentRunStatus.QUEUED
    assert parent.thread_id == ""


@pytest.mark.unit
def test_release_pin_leaves_run_pinned_to_live_runner(db, create_user, workspace, pod):
    alive = _make_runner(create_user, workspace, pod, "alive")
    # The runner is busy, so the pinned run legitimately waits for it.
    _make_run(create_user, workspace, pod, status=AgentRunStatus.RUNNING, runner=alive, age_s=3600)
    run = _make_run(create_user, workspace, pod, pinned_runner=alive, age_s=3600)

    assert tasks.release_pins_on_offline_runners() == 0

    run.refresh_from_db()
    assert run.pinned_runner_id == alive.id


@pytest.mark.unit
def test_release_pin_waits_out_the_grace(db, create_user, workspace, pod):
    # Silent for less than the grace: a restart or a network blip.
    blip = _make_runner(create_user, workspace, pod, "blip", status=RunnerStatus.OFFLINE, silent_for_s=120)
    run = _make_run(create_user, workspace, pod, pinned_runner=blip, age_s=3600)

    assert tasks.release_pins_on_offline_runners() == 0

    run.refresh_from_db()
    assert run.pinned_runner_id == blip.id


@pytest.mark.unit
def test_release_pin_skips_managed_runs(db, create_user, workspace, pod):
    gone = _make_runner(create_user, workspace, pod, "gone", status=RunnerStatus.OFFLINE, silent_for_s=3600)
    run = _make_run(
        create_user,
        workspace,
        pod,
        pinned_runner=gone,
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
        age_s=3600,
    )

    assert tasks.release_pins_on_offline_runners() == 0

    run.refresh_from_db()
    assert run.pinned_runner_id == gone.id


@pytest.mark.unit
@override_settings(RUNNER_OFFLINE_PIN_RELEASE_SECS=0)
def test_release_pin_disabled_by_setting(db, create_user, workspace, pod):
    gone = _make_runner(create_user, workspace, pod, "gone", status=RunnerStatus.OFFLINE, silent_for_s=3600)
    run = _make_run(create_user, workspace, pod, pinned_runner=gone, age_s=3600)

    assert tasks.release_pins_on_offline_runners() == 0

    run.refresh_from_db()
    assert run.pinned_runner_id == gone.id


# ---------------- fail_runs_on_offline_runners ----------------


@pytest.mark.unit
@pytest.mark.parametrize(
    "status",
    [
        AgentRunStatus.ASSIGNED,
        AgentRunStatus.RUNNING,
        AgentRunStatus.AWAITING_APPROVAL,
        AgentRunStatus.AWAITING_REAUTH,
    ],
)
def test_fail_run_on_long_silent_runner(db, create_user, workspace, pod, status):
    gone = _make_runner(create_user, workspace, pod, "gone", status=RunnerStatus.OFFLINE, silent_for_s=7200)
    run = _make_run(create_user, workspace, pod, status=status, runner=gone, age_s=7200)

    assert tasks.fail_runs_on_offline_runners() == 1

    run.refresh_from_db()
    assert run.status == AgentRunStatus.FAILED
    assert run.error_code == "runner_offline"
    assert run.ended_at is not None


@pytest.mark.unit
def test_fail_run_unblocks_the_issue(db, create_user, workspace, pod, issue):
    gone = _make_runner(create_user, workspace, pod, "gone", status=RunnerStatus.BUSY, silent_for_s=7200)
    run = _make_run(
        create_user, workspace, pod, status=AgentRunStatus.RUNNING, runner=gone, work_item=issue, age_s=7200
    )
    assert _active_run_for(issue) == run

    assert tasks.fail_runs_on_offline_runners() == 1

    assert _active_run_for(issue) is None


@pytest.mark.unit
def test_fail_run_for_runner_that_never_heartbeated(db, create_user, workspace, pod):
    never = _make_runner(create_user, workspace, pod, "never", status=RunnerStatus.OFFLINE, silent_for_s=None)
    run = _make_run(create_user, workspace, pod, status=AgentRunStatus.ASSIGNED, runner=never, age_s=7200)

    assert tasks.fail_runs_on_offline_runners() == 1

    run.refresh_from_db()
    assert run.status == AgentRunStatus.FAILED


@pytest.mark.unit
def test_fail_run_skips_managed_runs(db, create_user, workspace, pod):
    gone = _make_runner(create_user, workspace, pod, "gone", status=RunnerStatus.OFFLINE, silent_for_s=7200)
    run = _make_run(
        create_user,
        workspace,
        pod,
        status=AgentRunStatus.RUNNING,
        runner=gone,
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
        age_s=7200,
    )

    assert tasks.fail_runs_on_offline_runners() == 0

    run.refresh_from_db()
    assert run.status == AgentRunStatus.RUNNING


@pytest.mark.unit
def test_fail_run_rechecks_heartbeat_under_the_runner_lock(db, create_user, workspace, pod):
    gone = _make_runner(create_user, workspace, pod, "gone", status=RunnerStatus.OFFLINE, silent_for_s=7200)
    run = _make_run(create_user, workspace, pod, status=AgentRunStatus.RUNNING, runner=gone, age_s=7200)
    real = session_service._finish_unclaimed_runs

    # The runner polls after the sweep has listed it but before its turn.
    def _runner_returns_then_lock(qs_method):
        def wrapped(self, *args, **kwargs):
            Runner.objects.filter(pk=gone.pk).update(last_heartbeat_at=timezone.now())
            return qs_method(self, *args, **kwargs)

        return wrapped

    from django.db.models.query import QuerySet

    with patch.object(QuerySet, "select_for_update", _runner_returns_then_lock(QuerySet.select_for_update)):
        with patch.object(session_service, "_finish_unclaimed_runs", wraps=real) as finish:
            assert tasks.fail_runs_on_offline_runners() == 0

    finish.assert_not_called()
    run.refresh_from_db()
    assert run.status == AgentRunStatus.RUNNING


@pytest.mark.unit
def test_pending_cancellation_on_silent_runner_becomes_cancelled(db, create_user, workspace, pod):
    gone = _make_runner(create_user, workspace, pod, "gone", status=RunnerStatus.OFFLINE, silent_for_s=7200)
    run = _make_run(create_user, workspace, pod, status=AgentRunStatus.CANCEL_REQUESTED, runner=gone, age_s=7200)

    assert tasks.fail_runs_on_offline_runners() == 1

    run.refresh_from_db()
    assert run.status == AgentRunStatus.CANCELLED


@pytest.mark.unit
def test_fail_run_waits_out_the_grace(db, create_user, workspace, pod):
    # Ten minutes of silence: a sleeping laptop keeps its run.
    asleep = _make_runner(create_user, workspace, pod, "asleep", status=RunnerStatus.OFFLINE, silent_for_s=600)
    run = _make_run(create_user, workspace, pod, status=AgentRunStatus.RUNNING, runner=asleep, age_s=7200)

    assert tasks.fail_runs_on_offline_runners() == 0

    run.refresh_from_db()
    assert run.status == AgentRunStatus.RUNNING


@pytest.mark.unit
def test_fail_run_ignores_live_runner(db, create_user, workspace, pod):
    alive = _make_runner(create_user, workspace, pod, "alive")
    run = _make_run(create_user, workspace, pod, status=AgentRunStatus.RUNNING, runner=alive, age_s=7200)

    assert tasks.fail_runs_on_offline_runners() == 0

    run.refresh_from_db()
    assert run.status == AgentRunStatus.RUNNING


@pytest.mark.unit
def test_fail_run_frees_pod_queue_for_another_runner(db, create_user, workspace, pod):
    gone = _make_runner(create_user, workspace, pod, "gone", status=RunnerStatus.OFFLINE, silent_for_s=7200)
    alive = _make_runner(create_user, workspace, pod, "alive")
    # Nothing has drained the pod since ``queued`` was created; the sweep's
    # post-commit drain is what hands it to ``alive``.
    stranded = _make_run(create_user, workspace, pod, status=AgentRunStatus.RUNNING, runner=gone, age_s=7200)
    queued = _make_run(create_user, workspace, pod)

    assert tasks.fail_runs_on_offline_runners() == 1

    stranded.refresh_from_db()
    queued.refresh_from_db()
    assert stranded.status == AgentRunStatus.FAILED
    assert queued.status == AgentRunStatus.ASSIGNED
    assert queued.runner_id == alive.id


@pytest.mark.unit
@override_settings(RUNNER_OFFLINE_RUN_FAIL_SECS=0)
def test_fail_run_disabled_by_setting(db, create_user, workspace, pod):
    gone = _make_runner(create_user, workspace, pod, "gone", status=RunnerStatus.OFFLINE, silent_for_s=7200)
    run = _make_run(create_user, workspace, pod, status=AgentRunStatus.RUNNING, runner=gone, age_s=7200)

    assert tasks.fail_runs_on_offline_runners() == 0

    run.refresh_from_db()
    assert run.status == AgentRunStatus.RUNNING


# ---------------- returning daemon is told to stop ----------------


@pytest.mark.unit
def test_poll_cancels_run_the_offline_sweep_already_failed(db, create_user, workspace, pod, _stub_send_to_runner):
    asleep = _make_runner(create_user, workspace, pod, "asleep", status=RunnerStatus.OFFLINE, silent_for_s=7200)
    run = _make_run(create_user, workspace, pod, status=AgentRunStatus.RUNNING, runner=asleep, age_s=7200)
    assert tasks.fail_runs_on_offline_runners() == 1
    _stub_send_to_runner.reset_mock()

    # The laptop wakes and its daemon still reports the run as in flight.
    session_service.reap_stale_busy_runs(asleep, {"in_flight_run": str(run.id)})

    _stub_send_to_runner.assert_called_once_with(
        asleep.id,
        {"v": 1, "type": "cancel", "run_id": str(run.id), "reason": "run_already_failed"},
    )


@pytest.mark.unit
def test_poll_does_not_cancel_run_that_failed_for_another_reason(db, create_user, workspace, pod, _stub_send_to_runner):
    runner = _make_runner(create_user, workspace, pod, "alive")
    run = _make_run(
        create_user, workspace, pod, status=AgentRunStatus.FAILED, runner=runner, error_code="heartbeat_reaped"
    )

    session_service.reap_stale_busy_runs(runner, {"in_flight_run": str(run.id)})

    _stub_send_to_runner.assert_not_called()
