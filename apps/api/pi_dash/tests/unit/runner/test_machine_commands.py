# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Cloud-driven runner creation on a connected dev machine.

Covers the web-session enqueue endpoint (online enqueue, offline
fail-fast, validation), the daemon result write-back (including the
machine auth boundary), and the status poll loop the modal runs.
"""

from __future__ import annotations

import json

import pytest
from django.urls import reverse
from django.utils import timezone

from pi_dash.runner.models import DevMachine, MachineSession, MachineToken
from pi_dash.runner.services import machine_outbox
from pi_dash.runner.services import tokens


class _FakeRedis:
    """Minimal Redis stand-in: streams for the outbox, KV for results."""

    def __init__(self):
        self.streams: dict[str, list] = {}
        self.kv: dict[str, str] = {}
        self._seq = 0

    def xgroup_create(self, **_kwargs):
        return True

    def xadd(self, key, fields, maxlen=None, approximate=None):
        self._seq += 1
        sid = f"{self._seq}-0".encode()
        self.streams.setdefault(key, []).append((sid, dict(fields)))
        return sid

    def xrange(self, key):
        return self.streams.get(key, [])

    def xack(self, *_args):
        return 0

    def delete(self, *keys):
        for k in keys:
            self.kv.pop(k, None)
            self.streams.pop(k, None)
        return 0

    def expire(self, *_args):
        return True

    def set(self, key, val, ex=None):
        self.kv[key] = val
        return True

    def get(self, key):
        return self.kv.get(key)

    def exists(self, key):
        return 1 if key in self.kv else 0

    def publish(self, *_args):
        return 0


@pytest.fixture(autouse=True)
def _fake_redis(monkeypatch):
    client = _FakeRedis()
    monkeypatch.setattr(machine_outbox, "redis_instance", lambda: client)
    return client


@pytest.fixture
def dev_machine(create_user):
    return DevMachine.objects.create(owner=create_user, host_label="host-a")


@pytest.fixture
def machine_token(create_user, workspace, dev_machine):
    minted = tokens.mint_machine_token()
    MachineToken.objects.create(
        user=create_user,
        workspace=workspace,
        dev_machine=dev_machine,
        host_label="host-a",
        token_hash=minted.hashed,
        token_fingerprint=minted.fingerprint,
        label="machine: host-a",
        is_service=True,
    )
    return minted.raw


@pytest.fixture
def live_session(dev_machine):
    """An active machine control session — the machine counts as online."""
    return MachineSession.objects.create(
        dev_machine=dev_machine, last_seen_at=timezone.now()
    )


def _enqueue_url(machine_id):
    return reverse("dev-machine-create-runner", kwargs={"machine_id": machine_id})


def _status_url(machine_id, request_id):
    return reverse(
        "dev-machine-create-runner-status",
        kwargs={"machine_id": machine_id, "request_id": request_id},
    )


def _result_url(machine_id, request_id):
    return reverse(
        "runner:machine-command-result",
        kwargs={"dev_machine_id": machine_id, "request_id": request_id},
    )


def _enqueue(client, dev_machine, workspace, **overrides):
    body = {
        "workspace": str(workspace.id),
        "project": "DEF",
        "agent": "claude-code",
        **overrides,
    }
    return client.post(_enqueue_url(dev_machine.id), body, format="json")


# ---- Enqueue ---------------------------------------------------------------


@pytest.mark.unit
def test_enqueue_online_machine_puts_message_on_stream(
    db, session_client, workspace, dev_machine, machine_token, live_session, _fake_redis
):
    resp = _enqueue(
        session_client,
        dev_machine,
        workspace,
        name="my_runner",
        working_dir="/tmp/proj",
        model="claude-opus-4-8",
    )
    assert resp.status_code == 202, resp.data
    request_id = resp.data["request_id"]

    entries = _fake_redis.streams.get(machine_outbox.stream_key(dev_machine.id))
    assert entries and len(entries) == 1
    payload = json.loads(entries[0][1]["payload"])
    assert payload["type"] == "create_runner"
    assert payload["request_id"] == request_id
    assert payload["project"] == "DEF"
    assert payload["name"] == "my_runner"
    assert payload["working_dir"] == "/tmp/proj"
    assert payload["agent"] == "claude-code"
    assert payload["model"] == "claude-opus-4-8"
    assert payload["workspace_slug"] == workspace.slug

    # Pending marker is readable through the status endpoint.
    status_resp = session_client.get(
        _status_url(dev_machine.id, request_id), {"workspace": str(workspace.id)}
    )
    assert status_resp.status_code == 200
    assert status_resp.data["status"] == "pending"


@pytest.mark.unit
def test_enqueue_offline_machine_fails_fast(
    db, session_client, workspace, dev_machine, machine_token, _fake_redis
):
    # No active MachineSession → create_runner must offline-reject.
    resp = _enqueue(session_client, dev_machine, workspace)
    assert resp.status_code == 409, resp.data
    assert resp.data["error"] == "machine_offline"
    assert machine_outbox.offline_stream_key(dev_machine.id) not in _fake_redis.streams


@pytest.mark.unit
def test_enqueue_delivery_failure_returns_503(
    db, session_client, workspace, dev_machine, machine_token, live_session, _fake_redis
):
    # Redis write blows up mid-enqueue: the operator must get an
    # immediate 503, not a 202 that polls "pending" to timeout.
    def _boom(*_args, **_kwargs):
        raise RuntimeError("redis down")

    _fake_redis.xadd = _boom
    resp = _enqueue(session_client, dev_machine, workspace)
    assert resp.status_code == 503, resp.data
    assert resp.data["error"] == "delivery_failed"


@pytest.mark.unit
def test_enqueue_rejects_bad_runner_name(
    db, session_client, workspace, dev_machine, machine_token, live_session
):
    resp = _enqueue(session_client, dev_machine, workspace, name="has space")
    assert resp.status_code == 400
    assert resp.data["error"] == "invalid_runner_name"


@pytest.mark.unit
def test_enqueue_rejects_unknown_project(
    db, session_client, workspace, dev_machine, machine_token, live_session
):
    resp = _enqueue(session_client, dev_machine, workspace, project="NOPE")
    assert resp.status_code == 404
    assert resp.data["error"] == "project_not_found"


@pytest.mark.unit
def test_enqueue_rejects_unknown_agent(
    db, session_client, workspace, dev_machine, machine_token, live_session
):
    resp = _enqueue(session_client, dev_machine, workspace, agent="skynet")
    assert resp.status_code == 400
    assert resp.data["error"] == "invalid_agent"


@pytest.mark.unit
def test_enqueue_requires_workspace_scope(
    db, api_client, create_user, workspace, dev_machine, machine_token, live_session
):
    # Unauthenticated → 401/403 from DRF.
    resp = api_client.post(_enqueue_url(dev_machine.id), {"workspace": str(workspace.id), "project": "DEF"})
    assert resp.status_code in (401, 403)


@pytest.mark.unit
def test_enqueue_machine_not_in_workspace_scope_404s(
    db, session_client, create_user, workspace, live_session
):
    # A machine with no runner/token linkage to the workspace is invisible.
    stray = DevMachine.objects.create(owner=create_user, host_label="stray")
    resp = _enqueue(session_client, stray, workspace)
    assert resp.status_code == 404


# ---- Result write-back -----------------------------------------------------


@pytest.mark.unit
def test_result_writeback_roundtrip(
    db, session_client, api_client, workspace, dev_machine, machine_token, live_session
):
    resp = _enqueue(session_client, dev_machine, workspace)
    assert resp.status_code == 202
    request_id = resp.data["request_id"]

    # session_client and api_client share the underlying client;
    # force_authenticate would bypass MachineTokenAuthentication.
    api_client.force_authenticate(user=None)
    api_client.credentials(HTTP_AUTHORIZATION=f"Bearer {machine_token}")
    result = api_client.post(
        _result_url(dev_machine.id, request_id),
        {"status": "ok", "runner_id": "1e6a7a56-0000-0000-0000-000000000001", "runner_name": "runner_001"},
        format="json",
    )
    assert result.status_code == 204, result.data

    api_client.credentials()
    session_client.force_authenticate(user=workspace.owner)
    status_resp = session_client.get(
        _status_url(dev_machine.id, request_id), {"workspace": str(workspace.id)}
    )
    assert status_resp.status_code == 200
    assert status_resp.data["status"] == "ok"
    assert status_resp.data["runner_name"] == "runner_001"


@pytest.mark.unit
def test_result_writeback_rejects_other_machine_token(
    db, session_client, api_client, create_user, workspace, dev_machine, machine_token, live_session
):
    resp = _enqueue(session_client, dev_machine, workspace)
    request_id = resp.data["request_id"]

    other_machine = DevMachine.objects.create(owner=create_user, host_label="host-b")
    minted = tokens.mint_machine_token()
    MachineToken.objects.create(
        user=create_user,
        workspace=workspace,
        dev_machine=other_machine,
        host_label="host-b",
        token_hash=minted.hashed,
        token_fingerprint=minted.fingerprint,
        label="machine: host-b",
        is_service=True,
    )
    api_client.force_authenticate(user=None)
    api_client.credentials(HTTP_AUTHORIZATION=f"Bearer {minted.raw}")
    result = api_client.post(
        _result_url(dev_machine.id, request_id),
        {"status": "ok"},
        format="json",
    )
    assert result.status_code == 403
    assert result.data["error"] == "dev_machine_mismatch"

    # Machine binding: even a token correctly bound to machine B (URL and
    # token match) cannot report a result for a request issued FOR
    # machine A — reads as unknown so existence doesn't leak.
    result = api_client.post(
        _result_url(other_machine.id, request_id),
        {"status": "ok", "runner_name": "forged"},
        format="json",
    )
    assert result.status_code == 404
    assert result.data["error"] == "unknown_request"
    # And the original pending marker is untouched.
    stored = machine_outbox.get_command_result(request_id)
    assert stored["status"] == "pending"


@pytest.mark.unit
def test_result_writeback_unknown_request_404s(
    db, api_client, workspace, dev_machine, machine_token, live_session
):
    api_client.credentials(HTTP_AUTHORIZATION=f"Bearer {machine_token}")
    result = api_client.post(
        _result_url(dev_machine.id, "00000000-0000-0000-0000-000000000000"),
        {"status": "ok"},
        format="json",
    )
    assert result.status_code == 404
    assert result.data["error"] == "unknown_request"


@pytest.mark.unit
def test_status_read_is_machine_bound(
    db, session_client, create_user, workspace, dev_machine, machine_token, live_session
):
    resp = _enqueue(session_client, dev_machine, workspace)
    request_id = resp.data["request_id"]

    # Reading the result through a different machine's URL reads as
    # unknown, even for the same (authorized) web user.
    other_machine = DevMachine.objects.create(owner=create_user, host_label="host-c")
    minted = tokens.mint_machine_token()
    MachineToken.objects.create(
        user=create_user,
        workspace=workspace,
        dev_machine=other_machine,
        host_label="host-c",
        token_hash=minted.hashed,
        token_fingerprint=minted.fingerprint,
        label="machine: host-c",
        is_service=True,
    )
    status_resp = session_client.get(
        _status_url(other_machine.id, request_id), {"workspace": str(workspace.id)}
    )
    assert status_resp.status_code == 404

    # The right machine still reads it, without leaking the binding field.
    status_resp = session_client.get(
        _status_url(dev_machine.id, request_id), {"workspace": str(workspace.id)}
    )
    assert status_resp.status_code == 200
    assert status_resp.data["status"] == "pending"
    assert "dev_machine_id" not in status_resp.data


@pytest.mark.unit
def test_result_writeback_rejects_bad_status(
    db, session_client, api_client, workspace, dev_machine, machine_token, live_session
):
    resp = _enqueue(session_client, dev_machine, workspace)
    request_id = resp.data["request_id"]
    api_client.force_authenticate(user=None)
    api_client.credentials(HTTP_AUTHORIZATION=f"Bearer {machine_token}")
    result = api_client.post(
        _result_url(dev_machine.id, request_id),
        {"status": "sideways"},
        format="json",
    )
    assert result.status_code == 400


@pytest.mark.unit
def test_status_unknown_request_404s(
    db, session_client, workspace, dev_machine, machine_token
):
    status_resp = session_client.get(
        _status_url(dev_machine.id, "00000000-0000-0000-0000-000000000000"),
        {"workspace": str(workspace.id)},
    )
    assert status_resp.status_code == 404


# ---- control_online annotation --------------------------------------------


@pytest.mark.unit
def test_dev_machine_list_control_online_flag(
    db, session_client, workspace, dev_machine, machine_token, live_session
):
    url = reverse("dev-machine-list")
    resp = session_client.get(url, {"workspace": str(workspace.id)})
    assert resp.status_code == 200
    rows = {row["id"]: row for row in resp.data}
    assert rows[str(dev_machine.id)]["control_online"] is True

    # Revoking the session flips the flag off.
    MachineSession.objects.filter(dev_machine=dev_machine).update(
        revoked_at=timezone.now()
    )
    resp = session_client.get(url, {"workspace": str(workspace.id)})
    rows = {row["id"]: row for row in resp.data}
    assert rows[str(dev_machine.id)]["control_online"] is False
