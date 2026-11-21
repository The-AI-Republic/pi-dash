# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Dev-machine run-queue capacity HTTP contract (PDASHOSS01-372).

Wire-shape cases the NEWPIDASH-242 Rust child mirrors byte-for-byte
(see ``rust-api/contract-tests/README.md``): these assert the exact
JSON the daemon contract consumes — response keys, decision vocabulary,
error codes and the priority of eviction/token/status decisions — not
internal storage layout.
"""

from __future__ import annotations

import uuid as _uuid
from unittest.mock import patch

import pytest
from django.utils import timezone

from pi_dash.runner.models import (
    MACHINE_RUN_QUEUE_CAPABILITY,
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services import matcher, tokens

pytestmark = pytest.mark.contract

CAPACITY_KEYS = {"decision", "status", "assignment_token", "assignment_protocol", "revision"}


@pytest.fixture(autouse=True)
def _no_outbound(monkeypatch):
    monkeypatch.setattr("pi_dash.runner.services.pubsub.send_to_runner", lambda *a, **k: None)
    with patch("celery.app.task.Task.delay", return_value=None):
        yield


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
    return tokens.mint_access_token(
        runner_id=str(runner.id),
        user_id=str(runner.owner_id),
        workspace_id=str(runner.workspace_id),
        rtg=1,
    ).raw


@pytest.fixture
def queued(db, create_user, workspace, pod):
    return AgentRun.objects.create(
        owner=create_user,
        created_by=create_user,
        workspace=workspace,
        pod=pod,
        prompt="contract case",
        status=AgentRunStatus.QUEUED,
    )


def _open(api_client, runner, token, caps):
    return api_client.post(
        f"/api/v1/runner/runners/{runner.id}/sessions/",
        {"capabilities": caps},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {token}",
    )


def _capacity(api_client, token, run, body, key=None):
    headers = {"HTTP_AUTHORIZATION": f"Bearer {token}"}
    if key is not None:
        headers["HTTP_IDEMPOTENCY_KEY"] = key
    return api_client.post(f"/api/v1/runner/runs/{run.id}/capacity/", body, format="json", **headers)


def test_session_welcome_advertises_queue_capability(api_client, runner, runner_token):
    resp = _open(api_client, runner, runner_token, [MACHINE_RUN_QUEUE_CAPABILITY])
    assert resp.status_code == 201
    assert resp.data["welcome"]["capabilities"] == [MACHINE_RUN_QUEUE_CAPABILITY]


def test_assign_message_carries_attempt_identity(db, api_client, runner, runner_token, queued):
    opened = _open(api_client, runner, runner_token, [MACHINE_RUN_QUEUE_CAPABILITY])
    assert opened.status_code == 201
    assert matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    msg = matcher._build_assign_msg(queued)
    assert msg["assignment_token"] == str(queued.assignment_token)
    assert msg["assignment_protocol"] == 1


def test_waiting_wire_shape(api_client, runner, runner_token, queued):
    sid = _open(api_client, runner, runner_token, [MACHINE_RUN_QUEUE_CAPABILITY]).data["session_id"]
    matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    resp = _capacity(
        api_client,
        runner_token,
        queued,
        {
            "action": "waiting",
            "session_id": sid,
            "assignment_token": str(queued.assignment_token),
            "revision": 12,
            "queued_at": "2026-11-13T18:00:00Z",
            "queue_position": 2,
            "active": 3,
            "limit": 3,
        },
        key=str(_uuid.uuid4()),
    )
    assert resp.status_code == 200
    assert set(resp.data) == CAPACITY_KEYS, resp.data
    assert resp.data["decision"] == "recorded"
    assert resp.data["status"] == "assigned"
    assert resp.data["revision"] == 12


def test_admit_wire_shape_and_retry_stability(api_client, runner, runner_token, queued):
    sid = _open(api_client, runner, runner_token, [MACHINE_RUN_QUEUE_CAPABILITY]).data["session_id"]
    matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    key = str(_uuid.uuid4())
    body = {"action": "admit", "session_id": sid, "assignment_token": str(queued.assignment_token), "revision": 1}
    first = _capacity(api_client, runner_token, queued, body, key=key)
    assert first.status_code == 200
    assert set(first.data) == CAPACITY_KEYS | {"admission_id"}
    assert first.data["decision"] == "proceed"
    assert first.data["admission_id"] == key
    # Lost-response retry with the same key returns the same complete
    # current decision — never a cached/obsolete proceed.
    retry = _capacity(api_client, runner_token, queued, body, key=key)
    assert retry.data["decision"] == "proceed"
    assert retry.data["admission_id"] == key


def test_eviction_outranks_valid_credentials(api_client, runner, runner_token, queued):
    sid = _open(api_client, runner, runner_token, [MACHINE_RUN_QUEUE_CAPABILITY]).data["session_id"]
    matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    _open(api_client, runner, runner_token, [MACHINE_RUN_QUEUE_CAPABILITY])  # evicts sid
    resp = _capacity(
        api_client,
        runner_token,
        queued,
        {"action": "admit", "session_id": sid, "assignment_token": str(queued.assignment_token), "revision": 1},
        key=str(_uuid.uuid4()),
    )
    assert resp.status_code == 409
    assert resp.data == {"error": "session_evicted"}


def test_cancel_and_terminal_never_admit(api_client, runner, runner_token, queued):
    sid = _open(api_client, runner, runner_token, [MACHINE_RUN_QUEUE_CAPABILITY]).data["session_id"]
    matcher.drain_for_runner_by_id(runner.id)
    queued.refresh_from_db()
    body = {"action": "admit", "session_id": sid, "assignment_token": str(queued.assignment_token), "revision": 1}
    AgentRun.objects.filter(pk=queued.pk).update(status=AgentRunStatus.CANCEL_REQUESTED)
    assert _capacity(api_client, runner_token, queued, body, key=str(_uuid.uuid4())).data["decision"] == "cancel"
    AgentRun.objects.filter(pk=queued.pk).update(status=AgentRunStatus.COMPLETED)
    terminal = _capacity(api_client, runner_token, queued, body, key=str(_uuid.uuid4()))
    assert terminal.data["decision"] == "terminal"
    assert "admission_id" not in terminal.data
