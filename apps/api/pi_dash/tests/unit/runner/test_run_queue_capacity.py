# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Dev-machine run-queue capacity contract (T1+T2, PDASHOSS01-372).

Covers: attempt identity minting/preservation, legacy adoption via
reconcile, the capacity endpoint wire format and decisions, protocol-1
lifecycle fencing, poll held-token/capacity snapshots, reaper guards,
and the serializer projection. The HTTP wire shapes asserted here are
the contract the NEWPIDASH-242 Rust child mirrors.
"""

from __future__ import annotations

import threading
import uuid as _uuid
from datetime import timedelta

import pytest
from django.utils import timezone

from pi_dash.runner.models import (
    MACHINE_RUN_QUEUE_CAPABILITY,
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerSession,
    RunnerStatus,
)
from pi_dash.runner.services import matcher, session_service
from pi_dash.runner.serializers import AgentRunSerializer


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def runner(db, create_user, workspace, pod):
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="agentR",
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
        refresh_token_generation=1,
        enrolled_at=timezone.now(),
    )


@pytest.fixture
def runner_token(runner):
    from pi_dash.runner.services import tokens

    return tokens.mint_access_token(
        runner_id=str(runner.id),
        user_id=str(runner.owner_id),
        workspace_id=str(runner.workspace_id),
        rtg=1,
    ).raw


@pytest.fixture
def open_session(db, api_client, runner, runner_token):
    resp = api_client.post(
        f"/api/v1/runner/runners/{runner.id}/sessions/",
        {"capabilities": [MACHINE_RUN_QUEUE_CAPABILITY]},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )
    assert resp.status_code == 201, resp.data
    return resp


@pytest.fixture
def queued(db, create_user, workspace, pod):
    return AgentRun.objects.create(
        owner=create_user,
        created_by=create_user,
        workspace=workspace,
        pod=pod,
        prompt="do the thing",
        status=AgentRunStatus.QUEUED,
    )


def _capacity(api_client, token, run, body, key=None):
    headers = {"HTTP_AUTHORIZATION": f"Bearer {token}"}
    if key is not None:
        headers["HTTP_IDEMPOTENCY_KEY"] = key
    return api_client.post(f"/api/v1/runner/runs/{run.id}/capacity/", body, format="json", **headers)


def _wait_body(session_id, token, revision=1, **extra):
    return {
        "action": "waiting",
        "session_id": str(session_id),
        "assignment_token": str(token),
        "revision": revision,
        "queue_position": 2,
        "active": 3,
        "limit": 3,
        **extra,
    }


# ---------------------------------------------------------------------------
# T1: attempt identity
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_machine_queue_capability_mints_protocol_one(db, runner, queued, open_session):
    sess = RunnerSession.objects.get(runner=runner, revoked_at__isnull=True)
    assert MACHINE_RUN_QUEUE_CAPABILITY in (sess.capabilities or [])
    welcome_caps = open_session.data["welcome"]["capabilities"]
    assert MACHINE_RUN_QUEUE_CAPABILITY in welcome_caps

    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    assert queued.status == AgentRunStatus.ASSIGNED
    assert queued.assignment_protocol == 1
    assert queued.assignment_token is not None
    assert queued.capacity_wait is None
    msg = matcher._build_assign_msg(queued)
    assert msg["assignment_token"] == str(queued.assignment_token)
    assert msg["assignment_protocol"] == 1


@pytest.mark.unit
def test_no_capability_assigns_protocol_zero(db, runner, queued):
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    assert queued.assignment_protocol == 0
    assert queued.assignment_token is not None


@pytest.mark.unit
def test_reassign_mints_fresh_token(db, runner, queued, open_session):
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    first_token = str(queued.assignment_token)

    # Requeue (attempt ends), then drain again: fresh token, cleared wait.
    from pi_dash.runner.services import run_lifecycle

    queued.capacity_wait = {"phase": "waiting_for_capacity", "revision": 5}
    queued.save(update_fields=["capacity_wait"])
    run_lifecycle.apply_run_resume_unavailable(runner, queued.id)
    queued.refresh_from_db()
    assert queued.status == AgentRunStatus.QUEUED
    assert queued.capacity_wait is None

    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    assert queued.assignment_protocol == 1
    assert str(queued.assignment_token) != first_token


@pytest.mark.unit
def test_redelivery_preserves_token(db, runner, queued, open_session):
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    before = (str(queued.assignment_token), queued.assignment_protocol)
    payload = session_service.build_session_open_redeliver(runner, None)
    assert payload is not None
    assert (payload["assignment_token"], payload["assignment_protocol"]) == before


# ---------------------------------------------------------------------------
# T2: capacity endpoint wire + decisions
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_waiting_records_and_returns_decision(db, api_client, runner, runner_token, queued, open_session):
    sid = open_session.data["session_id"]
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    resp = _capacity(api_client, runner_token, queued, _wait_body(sid, queued.assignment_token), key=str(_uuid.uuid4()))
    assert resp.status_code == 200, resp.data
    body = resp.data
    assert body["decision"] == "recorded"
    assert body["status"] == AgentRunStatus.ASSIGNED
    assert body["assignment_token"] == str(queued.assignment_token)
    assert body["assignment_protocol"] == 1
    assert body["revision"] == 1
    queued.refresh_from_db()
    cw = queued.capacity_wait
    assert cw["phase"] == "waiting_for_capacity"
    assert cw["queue_position"] == 2
    assert cw["active"] == 3
    assert cw["limit"] == 3
    assert "admission_id" not in cw


@pytest.mark.unit
def test_capacity_requires_idempotency_key(db, api_client, runner, runner_token, queued, open_session):
    sid = open_session.data["session_id"]
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    resp = _capacity(api_client, runner_token, queued, _wait_body(sid, queued.assignment_token))
    assert resp.status_code == 400
    assert resp.data["error"] == "idempotency_key_required"


@pytest.mark.unit
def test_capacity_body_limit(db, api_client, runner, runner_token, queued, open_session):
    sid = open_session.data["session_id"]
    body = _wait_body(sid, str(_uuid.uuid4()))
    body["pad"] = "x" * (16 * 1024)
    resp = _capacity(api_client, runner_token, queued, body, key=str(_uuid.uuid4()))
    assert resp.status_code == 400
    assert resp.data["error"] == "body_too_large"


@pytest.mark.unit
def test_capacity_rejects_invalid_revision(db, api_client, runner, runner_token, queued, open_session):
    sid = open_session.data["session_id"]
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    for bad in (-1, 1.5, True, "12", None):
        resp = _capacity(
            api_client,
            runner_token,
            queued,
            _wait_body(sid, queued.assignment_token, revision=bad),
            key=str(_uuid.uuid4()),
        )
        assert resp.status_code == 400, (bad, resp.data)
        assert resp.data["error"] == "revision_invalid"


@pytest.mark.unit
def test_capacity_stale_revision(db, api_client, runner, runner_token, queued, open_session):
    sid = open_session.data["session_id"]
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    first = _capacity(
        api_client, runner_token, queued, _wait_body(sid, queued.assignment_token, revision=5), key=str(_uuid.uuid4())
    )
    assert first.data["decision"] == "recorded"
    late = _capacity(
        api_client, runner_token, queued, _wait_body(sid, queued.assignment_token, revision=3), key=str(_uuid.uuid4())
    )
    assert late.data["decision"] == "stale_report"
    queued.refresh_from_db()
    assert queued.capacity_wait["revision"] == 5


@pytest.mark.unit
def test_capacity_wrong_token_is_stale(db, api_client, runner, runner_token, queued, open_session):
    sid = open_session.data["session_id"]
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    resp = _capacity(api_client, runner_token, queued, _wait_body(sid, str(_uuid.uuid4())), key=str(_uuid.uuid4()))
    assert resp.data["decision"] == "stale_assignment"
    queued.refresh_from_db()
    assert queued.capacity_wait is None


@pytest.mark.unit
def test_capacity_evicted_session_rejected(db, api_client, runner, runner_token, queued, open_session):
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    dead = str(_uuid.uuid4())
    resp = _capacity(
        api_client, runner_token, queued, _wait_body(dead, queued.assignment_token), key=str(_uuid.uuid4())
    )
    assert resp.status_code == 409
    assert resp.data["error"] == "session_evicted"


@pytest.mark.unit
def test_admit_proceed_same_id_on_retry(db, api_client, runner, runner_token, queued, open_session):
    sid = open_session.data["session_id"]
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    admit_key = str(_uuid.uuid4())
    body = {"action": "admit", "session_id": sid, "assignment_token": str(queued.assignment_token), "revision": 2}
    first = _capacity(api_client, runner_token, queued, body, key=admit_key)
    assert first.data["decision"] == "proceed"
    assert first.data["admission_id"] == admit_key
    queued.refresh_from_db()
    assert queued.status == AgentRunStatus.ASSIGNED
    assert queued.capacity_wait["phase"] == "preparing"
    assert queued.capacity_wait["queue_position"] is None

    # Lost-response retry: same key revalidates to the same admission.
    retry = _capacity(api_client, runner_token, queued, body, key=admit_key)
    assert retry.data["decision"] == "proceed"
    assert retry.data["admission_id"] == admit_key

    # A later request cannot overwrite the admission.
    second = _capacity(api_client, runner_token, queued, dict(body, revision=3), key=str(_uuid.uuid4()))
    assert second.data["decision"] == "stale_report"

    # Delayed waiting never downgrades preparing.
    delayed = _capacity(
        api_client, runner_token, queued, _wait_body(sid, queued.assignment_token, revision=9), key=str(_uuid.uuid4())
    )
    assert delayed.data["decision"] == "recorded"
    queued.refresh_from_db()
    assert queued.capacity_wait["phase"] == "preparing"
    assert queued.capacity_wait["admission_id"] == admit_key

    # Defer with the exact admission returns to waiting.
    deferred = _capacity(
        api_client,
        runner_token,
        queued,
        dict(body, action="defer", admission_id=admit_key, revision=10),
        key=str(_uuid.uuid4()),
    )
    assert deferred.data["decision"] == "recorded"
    queued.refresh_from_db()
    assert queued.capacity_wait["phase"] == "waiting_for_capacity"
    assert "admission_id" not in queued.capacity_wait


@pytest.mark.unit
def test_terminal_cancel_started_decisions_cannot_launch(db, api_client, runner, runner_token, queued, open_session):
    sid = open_session.data["session_id"]
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    admit = {"action": "admit", "session_id": sid, "assignment_token": str(queued.assignment_token), "revision": 2}
    key = str(_uuid.uuid4())

    AgentRun.objects.filter(pk=queued.pk).update(status=AgentRunStatus.CANCEL_REQUESTED)
    cancelled = _capacity(api_client, runner_token, queued, admit, key=key)
    assert cancelled.data["decision"] == "cancel"

    AgentRun.objects.filter(pk=queued.pk).update(status=AgentRunStatus.RUNNING)
    started = _capacity(api_client, runner_token, queued, admit, key=str(_uuid.uuid4()))
    assert started.data["decision"] == "already_started"

    AgentRun.objects.filter(pk=queued.pk).update(status=AgentRunStatus.COMPLETED)
    terminal = _capacity(api_client, runner_token, queued, admit, key=str(_uuid.uuid4()))
    assert terminal.data["decision"] == "terminal"
    queued.refresh_from_db()
    assert queued.capacity_wait is None


@pytest.mark.unit
def test_reconcile_retains_never_started(db, api_client, runner, runner_token, queued, open_session):
    sid = open_session.data["session_id"]
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    body = {"action": "reconcile", "session_id": sid, "assignment_token": str(queued.assignment_token), "revision": 1}
    resp = _capacity(api_client, runner_token, queued, body, key=str(_uuid.uuid4()))
    assert resp.data["decision"] == "retain"
    queued.refresh_from_db()
    assert queued.status == AgentRunStatus.ASSIGNED


# ---------------------------------------------------------------------------
# Legacy adoption (reconcile only)
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_legacy_reconcile_adopts_assigned(db, api_client, runner, runner_token, queued):
    # No session capabilities: protocol 0.
    open_resp = api_client.post(
        f"/api/v1/runner/runners/{runner.id}/sessions/",
        {"version": "v"},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )
    sid = open_resp.data["session_id"]
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    assert queued.assignment_protocol == 0

    # Non-reconcile actions are refused as in-flight.
    waiting = _capacity(
        api_client,
        runner_token,
        queued,
        dict(_wait_body(sid, queued.assignment_token), assignment_token=None),
        key=str(_uuid.uuid4()),
    )
    assert waiting.data["decision"] == "legacy_in_flight"

    body = {"action": "reconcile", "session_id": sid, "revision": 1}
    adopted = _capacity(api_client, runner_token, queued, body, key=str(_uuid.uuid4()))
    assert adopted.data["decision"] == "retain"
    assert adopted.data["assignment_protocol"] == 1
    assert adopted.data["assignment_token"] is not None
    assert adopted.data["revision"] is None
    queued.refresh_from_db()
    assert queued.assignment_protocol == 1
    assert queued.capacity_wait is None


@pytest.mark.unit
def test_legacy_reconcile_never_adopts_running(db, api_client, runner, runner_token, queued):
    open_resp = api_client.post(
        f"/api/v1/runner/runners/{runner.id}/sessions/",
        {"version": "v"},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )
    sid = open_resp.data["session_id"]
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    AgentRun.objects.filter(pk=queued.pk).update(status=AgentRunStatus.RUNNING)
    body = {"action": "reconcile", "session_id": sid, "revision": 1}
    resp = _capacity(api_client, runner_token, queued, body, key=str(_uuid.uuid4()))
    assert resp.data["decision"] == "already_started"
    queued.refresh_from_db()
    assert queued.assignment_protocol == 0


# ---------------------------------------------------------------------------
# Protocol-1 lifecycle fencing
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_tokenless_lifecycle_cannot_mutate_protocol_one(db, api_client, runner, runner_token, queued, open_session):
    sid = open_session.data["session_id"]
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    resp = api_client.post(
        f"/api/v1/runner/runs/{queued.id}/accept/",
        {"session_id": sid},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )
    assert resp.status_code == 409
    assert resp.data["error"] == "stale_assignment"
    queued.refresh_from_db()
    assert queued.status == AgentRunStatus.ASSIGNED


@pytest.mark.unit
def test_accept_requires_current_session(db, api_client, runner, runner_token, queued, open_session):
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    dead = str(_uuid.uuid4())
    resp = api_client.post(
        f"/api/v1/runner/runs/{queued.id}/accept/",
        {"session_id": dead, "assignment_token": str(queued.assignment_token)},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )
    assert resp.status_code == 409
    assert resp.data["error"] == "session_evicted"
    queued.refresh_from_db()
    assert queued.status == AgentRunStatus.ASSIGNED


@pytest.mark.unit
def test_accept_with_valid_attempt_proceeds_and_clears_wait(db, api_client, runner, runner_token, queued, open_session):
    sid = open_session.data["session_id"]
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    waiting = _capacity(
        api_client, runner_token, queued, _wait_body(sid, queued.assignment_token), key=str(_uuid.uuid4())
    )
    assert waiting.data["decision"] == "recorded"
    resp = api_client.post(
        f"/api/v1/runner/runs/{queued.id}/accept/",
        {"session_id": sid, "assignment_token": str(queued.assignment_token)},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )
    assert resp.status_code == 200, resp.data
    queued.refresh_from_db()
    assert queued.status == AgentRunStatus.RUNNING
    assert queued.capacity_wait is None


@pytest.mark.unit
def test_protocol_zero_lifecycle_unchanged(db, api_client, runner, runner_token, queued):
    api_client.post(
        f"/api/v1/runner/runners/{runner.id}/sessions/",
        {"version": "v"},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    assert queued.assignment_protocol == 0
    resp = api_client.post(
        f"/api/v1/runner/runs/{queued.id}/accept/",
        {},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )
    assert resp.status_code == 200, resp.data
    queued.refresh_from_db()
    assert queued.status == AgentRunStatus.RUNNING


# ---------------------------------------------------------------------------
# Serializer projection
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_serializer_projects_presentation_only(db, queued):
    queued.capacity_wait = {
        "phase": "waiting_for_capacity",
        "assignment_token": str(_uuid.uuid4()),
        "session_id": str(_uuid.uuid4()),
        "revision": 4,
        "queued_at": "2026-11-13T18:00:00Z",
        "reported_at": "2026-11-13T18:00:30Z",
        "queue_position": 2,
        "active": 3,
        "limit": 3,
    }
    queued.save(update_fields=["capacity_wait"])
    data = AgentRunSerializer(queued).data
    assert set(data["capacity_wait"]) == {"phase", "queue_position", "queued_at", "reported_at", "active", "limit"}
    queued.capacity_wait = None
    queued.save(update_fields=["capacity_wait"])
    assert AgentRunSerializer(queued.refresh_from_db() or queued).data["capacity_wait"] is None


# ---------------------------------------------------------------------------
# Reaper guards
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_stall_watchdog_excludes_fresh_wait(db, runner):
    from pi_dash.runner.models import RunnerLiveState
    from pi_dash.runner.tasks import reconcile_stalled_runs

    now = timezone.now()
    run = AgentRun.objects.create(
        owner=runner.owner,
        created_by=runner.owner,
        workspace=runner.workspace,
        pod=runner.pod,
        runner=runner,
        prompt="x",
        status=AgentRunStatus.ASSIGNED,
        assigned_at=now - timedelta(seconds=600),
    )
    run.assignment_token = _uuid.uuid4()
    run.assignment_protocol = 1
    run.capacity_wait = {
        "phase": "waiting_for_capacity",
        "assignment_token": str(run.assignment_token),
        "revision": 2,
        "reported_at": timezone.now().isoformat(),
    }
    run.save()
    RunnerLiveState.objects.create(
        runner=runner,
        observed_run_id=run.id,
        last_event_at=now - timedelta(seconds=100000),
        updated_at=now,
    )
    assert reconcile_stalled_runs() == 0
    run.refresh_from_db()
    assert run.status == AgentRunStatus.ASSIGNED

    # Stale report no longer protects the run.
    run.capacity_wait = dict(run.capacity_wait, reported_at=(now - timedelta(seconds=600)).isoformat())
    run.save(update_fields=["capacity_wait"])
    assert reconcile_stalled_runs() == 1
    run.refresh_from_db()
    assert run.status == AgentRunStatus.FAILED


# ---------------------------------------------------------------------------
# Concurrent session-open / admit / poll / cancel
# ---------------------------------------------------------------------------


@pytest.mark.django_db(transaction=True)
def test_concurrent_session_open_admit_poll_cancel(db, runner, runner_token, project, create_user, workspace, pod):
    """Concurrent session replacement, admission, polls and cancel are safe.

    No request may 500; the surviving session is exactly one; the run
    ends in exactly one of the legal dispositions (assigned/cancelled)
    with a consistent capacity/admission state.
    """
    from rest_framework.test import APIClient

    queued = AgentRun.objects.create(
        owner=create_user,
        created_by=create_user,
        workspace=workspace,
        pod=pod,
        prompt="concurrent",
        status=AgentRunStatus.QUEUED,
    )
    open_resp = APIClient().post(
        f"/api/v1/runner/runners/{runner.id}/sessions/",
        {"capabilities": [MACHINE_RUN_QUEUE_CAPABILITY]},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {runner_token}",
    )
    assert open_resp.status_code == 201
    sid = open_resp.data["session_id"]
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    token = str(queued.assignment_token)

    results = []

    def client():
        return APIClient()

    def _record(exc):
        # Serialization aborts (lock timeouts, deadlocks) surface as
        # OperationalError in the thread; real callers get a bounded
        # 503 there. Record them as 503 so the assertion below sees
        # the retryable code. Anything else is a genuine failure.
        from django.db.utils import OperationalError

        if isinstance(exc, OperationalError):
            results.append(503)
        else:
            results.append(exc)

    def do_poll(i):
        # The poll HTTP endpoint is ``async def`` (long-poll wait on the
        # event loop); drive its sync bookkeeping write path directly
        # from the threads. The Redis stream read contributes no state
        # to the contention under test.
        from pi_dash.runner.views import sessions as sessions_views

        try:
            err, _plan = sessions_views._poll_bookkeeping(
                runner,
                {
                    "ack": [],
                    "status": {"status": "busy", "in_flight_run": str(queued.id), "held_assignment_token": token},
                },
                sid,
            )
            results.append(200 if err is None else err["status"])
        except Exception as exc:  # pragma: no cover - surface as failure
            _record(exc)

    def do_admit(i):
        try:
            results.append(
                _capacity(
                    client(),
                    runner_token,
                    queued,
                    {"action": "admit", "session_id": sid, "assignment_token": token, "revision": 1},
                    key=str(_uuid.uuid4()),
                ).status_code
            )
        except Exception as exc:  # pragma: no cover
            _record(exc)

    def do_reopen(i):
        try:
            results.append(
                client()
                .post(
                    f"/api/v1/runner/runners/{runner.id}/sessions/",
                    {"capabilities": [MACHINE_RUN_QUEUE_CAPABILITY]},
                    format="json",
                    HTTP_AUTHORIZATION=f"Bearer {runner_token}",
                )
                .status_code
            )
        except Exception as exc:  # pragma: no cover
            _record(exc)

    def do_cancel(i):
        try:
            AgentRun.objects.filter(pk=queued.pk).update(status=AgentRunStatus.CANCEL_REQUESTED)
            results.append("cancelled")
        except Exception as exc:  # pragma: no cover
            _record(exc)

    threads = [
        threading.Thread(target=fn, args=(i,))
        for i, fn in enumerate([do_poll, do_admit, do_reopen, do_poll, do_cancel, do_admit])
    ]
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=60)
    # No unexpected failures under contention. 409s are the expected
    # session-evicted rejections once the reopen evicted the original
    # session — even with valid runner credentials; 503s are the
    # bounded lock-timeout backoff under runner-row contention.
    assert all(code == "cancelled" or (isinstance(code, int) and code in (200, 201, 409, 503)) for code in results), (
        results
    )
    active = list(RunnerSession.objects.filter(runner=runner, revoked_at__isnull=True).values_list("id", flat=True))
    assert len(active) == 1, active
    queued.refresh_from_db()
    # The storm left a consistent, usable state: a fresh session
    # admits successfully (decision cancel, since do_cancel ran).
    fresh_sid = str(active[0])
    final = _capacity(
        client(),
        runner_token,
        queued,
        {"action": "admit", "session_id": fresh_sid, "assignment_token": token, "revision": 99},
        key=str(_uuid.uuid4()),
    )
    assert final.status_code == 200, final.data
    assert final.data["decision"] == "cancel"
    queued.refresh_from_db()
    assert queued.status == AgentRunStatus.CANCEL_REQUESTED
