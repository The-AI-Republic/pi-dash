# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the per-active-run agent observability snapshot.

Covers:

- ``upsert_runner_live_state`` ingestion semantics (new row, same-run
  update, ``observed_run_id`` change wipe, idle clear, malformed UUID,
  pre-observability runner is a no-op).
- ``reconcile_stalled_runs`` watchdog matches only when the snapshot's
  ``observed_run_id`` equals the run's id, ``updated_at`` is fresh, and
  ``last_event_at`` is stale.
- Pre-observability poll bodies leave any pre-existing
  ``RunnerLiveState`` row untouched.

See ``.ai_design/runner_agent_bridge/design.md`` §4.5.2 / §4.5.3.
"""

from __future__ import annotations

import uuid
from datetime import timedelta
from unittest.mock import patch

import pytest
from django.test import override_settings
from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerLiveState,
    RunnerStatus,
)
from pi_dash.runner.services.session_service import upsert_runner_live_state
from pi_dash.runner.tasks import reconcile_stalled_runs


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


def _make_runner(user, workspace, pod, name="r1"):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


def _make_run(user, workspace, pod, runner, *, status=AgentRunStatus.RUNNING):
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


@pytest.fixture(autouse=True)
def _run_on_commit_immediately():
    with patch(
        "django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()
    ):
        yield


@pytest.fixture(autouse=True)
def _stub_send_to_runner():
    with patch(
        "pi_dash.runner.services.pubsub.send_to_runner"
    ) as mock:
        yield mock


# ---------------------------------------------------------------------------
# upsert_runner_live_state
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_upsert_no_op_for_pre_observability_poll(
    db, create_user, workspace, pod
):
    """A poll body with no observability fields must not create a row."""
    runner = _make_runner(create_user, workspace, pod)
    # No observed_run_id, no scalar fields, no tokens.
    upsert_runner_live_state(runner, {"status": "busy", "ts": "2026-01-01T00:00:00Z"})
    assert not RunnerLiveState.objects.filter(runner=runner).exists()


@pytest.mark.unit
def test_upsert_creates_row_on_first_snapshot(
    db, create_user, workspace, pod
):
    runner = _make_runner(create_user, workspace, pod)
    rid = uuid.uuid4()
    now = timezone.now()
    upsert_runner_live_state(
        runner,
        {
            "observed_run_id": str(rid),
            "last_event_at": now.isoformat(),
            "last_event_kind": "codex/event/token_count",
            "last_event_summary": "tokens 100/200",
            "agent_pid": 4242,
            "agent_subprocess_alive": True,
            "approvals_pending": 0,
            "tokens": {"input": 100, "output": 200, "total": 300},
            "turn_count": 1,
        },
    )
    state = RunnerLiveState.objects.get(runner=runner)
    assert state.observed_run_id == rid
    assert state.last_event_kind == "codex/event/token_count"
    assert state.agent_pid == 4242
    assert state.agent_subprocess_alive is True
    assert state.approvals_pending == 0
    assert state.input_tokens == 100
    assert state.output_tokens == 200
    assert state.total_tokens == 300
    assert state.turn_count == 1


@pytest.mark.unit
def test_upsert_partial_update_preserves_unsent_fields(
    db, create_user, workspace, pod
):
    """A poll that omits a field must NOT NULL out the existing value."""
    runner = _make_runner(create_user, workspace, pod)
    rid = uuid.uuid4()
    upsert_runner_live_state(
        runner,
        {
            "observed_run_id": str(rid),
            "agent_pid": 4242,
            "tokens": {"input": 100, "output": 200, "total": 300},
            "turn_count": 1,
        },
    )
    # Subsequent poll updates only turn_count; tokens / pid must persist.
    upsert_runner_live_state(
        runner,
        {
            "observed_run_id": str(rid),
            "turn_count": 2,
        },
    )
    state = RunnerLiveState.objects.get(runner=runner)
    assert state.turn_count == 2
    assert state.agent_pid == 4242
    assert state.input_tokens == 100
    assert state.total_tokens == 300


@pytest.mark.unit
def test_upsert_run_id_change_wipes_snapshot_then_applies_incoming(
    db, create_user, workspace, pod
):
    runner = _make_runner(create_user, workspace, pod)
    rid_a = uuid.uuid4()
    upsert_runner_live_state(
        runner,
        {
            "observed_run_id": str(rid_a),
            "agent_pid": 1111,
            "turn_count": 5,
            "tokens": {"input": 10, "output": 20, "total": 30},
        },
    )
    # Run B begins. Carries some new fields; the *unspecified* fields
    # from run A's row must be wiped, not preserved.
    rid_b = uuid.uuid4()
    upsert_runner_live_state(
        runner,
        {
            "observed_run_id": str(rid_b),
            "agent_pid": 2222,
            # No turn_count, no tokens — those must NOT carry over.
        },
    )
    state = RunnerLiveState.objects.get(runner=runner)
    assert state.observed_run_id == rid_b
    assert state.agent_pid == 2222
    assert state.turn_count is None
    assert state.input_tokens is None
    assert state.output_tokens is None
    assert state.total_tokens is None


@pytest.mark.unit
def test_upsert_idle_clears_run_binding(
    db, create_user, workspace, pod
):
    """An explicit observed_run_id=null clears the row's run binding."""
    runner = _make_runner(create_user, workspace, pod)
    rid = uuid.uuid4()
    upsert_runner_live_state(
        runner,
        {
            "observed_run_id": str(rid),
            "agent_pid": 4242,
            "turn_count": 3,
        },
    )
    upsert_runner_live_state(runner, {"observed_run_id": None})
    state = RunnerLiveState.objects.get(runner=runner)
    assert state.observed_run_id is None
    # All snapshot fields are wiped because the rid changed (Some → None).
    assert state.agent_pid is None
    assert state.turn_count is None


@pytest.mark.unit
def test_upsert_malformed_observed_run_id_is_ignored(
    db, create_user, workspace, pod
):
    runner = _make_runner(create_user, workspace, pod)
    # Pre-existing valid row.
    valid_rid = uuid.uuid4()
    upsert_runner_live_state(
        runner,
        {"observed_run_id": str(valid_rid), "agent_pid": 4242},
    )
    # Bad poll: invalid UUID. Must not raise; must leave the row intact.
    upsert_runner_live_state(
        runner,
        {"observed_run_id": "not-a-uuid", "agent_pid": 9999},
    )
    state = RunnerLiveState.objects.get(runner=runner)
    assert state.observed_run_id == valid_rid
    assert state.agent_pid == 4242  # unchanged


@pytest.mark.unit
def test_upsert_handles_get_or_create_for_returning_runner(
    db, create_user, workspace, pod
):
    """Subsequent polls keep updating the same row."""
    runner = _make_runner(create_user, workspace, pod)
    rid = uuid.uuid4()
    upsert_runner_live_state(runner, {"observed_run_id": str(rid)})
    upsert_runner_live_state(
        runner, {"observed_run_id": str(rid), "agent_pid": 1234}
    )
    assert RunnerLiveState.objects.filter(runner=runner).count() == 1


# ---------------------------------------------------------------------------
# reconcile_stalled_runs watchdog
# ---------------------------------------------------------------------------


@pytest.mark.unit
@override_settings(
    RUNNER_AGENT_STALL_THRESHOLD_SECS=300,
    RUNNER_AGENT_OBSERVABILITY_STALE_SECS=90,
)
def test_watchdog_reaps_when_all_three_conditions_hold(
    db, create_user, workspace, pod
):
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner)
    now = timezone.now()
    # Snapshot says "this run, fresh poll, agent silent for >5min".
    state = RunnerLiveState.objects.create(
        runner=runner,
        observed_run_id=run.id,
        last_event_at=now - timedelta(seconds=600),
    )
    # `auto_now=True` re-stamps `updated_at` on every save(), so we have
    # to bypass the ORM with a raw UPDATE to set a deterministic value.
    # The fresh-row state we want IS roughly "now", but pinning it
    # avoids the slim race where test setup + the watchdog's `now`
    # straddle a tick boundary.
    RunnerLiveState.objects.filter(pk=state.pk).update(
        updated_at=now - timedelta(seconds=10)
    )

    reaped = reconcile_stalled_runs()
    assert reaped == 1
    run.refresh_from_db()
    assert run.status == AgentRunStatus.FAILED
    assert "agent stalled" in run.error
    assert "300s" in run.error


@pytest.mark.unit
@override_settings(RUNNER_AGENT_STALL_THRESHOLD_SECS=300)
def test_watchdog_does_not_reap_when_observed_run_id_mismatches(
    db, create_user, workspace, pod
):
    """A previous run's snapshot must not fail the *current* run."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner)
    other_rid = uuid.uuid4()  # snapshot describes a different run
    RunnerLiveState.objects.create(
        runner=runner,
        observed_run_id=other_rid,
        last_event_at=timezone.now() - timedelta(seconds=600),
    )

    reaped = reconcile_stalled_runs()
    assert reaped == 0
    run.refresh_from_db()
    assert run.status == AgentRunStatus.RUNNING


@pytest.mark.unit
@override_settings(
    RUNNER_AGENT_STALL_THRESHOLD_SECS=300,
    RUNNER_AGENT_OBSERVABILITY_STALE_SECS=90,
)
def test_watchdog_does_not_reap_when_snapshot_row_is_stale(
    db, create_user, workspace, pod
):
    """If the runner stops reporting, age-out instead of failing run."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner)
    now = timezone.now()
    state = RunnerLiveState.objects.create(
        runner=runner,
        observed_run_id=run.id,
        last_event_at=now - timedelta(seconds=600),
    )
    # Force updated_at to be older than the freshness cutoff.
    RunnerLiveState.objects.filter(pk=state.pk).update(
        updated_at=now - timedelta(seconds=300)
    )

    reaped = reconcile_stalled_runs()
    assert reaped == 0


@pytest.mark.unit
def test_watchdog_does_not_reap_run_without_live_state_row(
    db, create_user, workspace, pod
):
    runner = _make_runner(create_user, workspace, pod)
    _make_run(create_user, workspace, pod, runner)
    # No live_state row at all (pre-observability runner).
    reaped = reconcile_stalled_runs()
    assert reaped == 0


@pytest.mark.unit
@override_settings(
    RUNNER_AGENT_STALL_THRESHOLD_SECS=300,
    RUNNER_AGENT_OBSERVABILITY_STALE_SECS=90,
)
def test_watchdog_does_not_reap_awaiting_approval(
    db, create_user, workspace, pod
):
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(
        create_user,
        workspace,
        pod,
        runner,
        status=AgentRunStatus.AWAITING_APPROVAL,
    )
    now = timezone.now()
    state = RunnerLiveState.objects.create(
        runner=runner,
        observed_run_id=run.id,
        last_event_at=now - timedelta(seconds=600),
    )
    # Bypass auto_now=True with a raw UPDATE — see the fresh-row test.
    RunnerLiveState.objects.filter(pk=state.pk).update(
        updated_at=now - timedelta(seconds=10)
    )

    reaped = reconcile_stalled_runs()
    assert reaped == 0
    run.refresh_from_db()
    assert run.status == AgentRunStatus.AWAITING_APPROVAL


@pytest.mark.unit
def test_watchdog_does_not_reap_pre_observability_run_with_null_last_event_at(
    db, create_user, workspace, pod
):
    """NULL last_event_at is excluded by `__lt` — pre-flag runs stay safe."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner)
    now = timezone.now()
    # Row exists but last_event_at is NULL.
    state = RunnerLiveState.objects.create(
        runner=runner, observed_run_id=run.id, last_event_at=None
    )
    # Bypass auto_now=True with a raw UPDATE — see the fresh-row test.
    RunnerLiveState.objects.filter(pk=state.pk).update(
        updated_at=now - timedelta(seconds=5)
    )
    reaped = reconcile_stalled_runs()
    assert reaped == 0
