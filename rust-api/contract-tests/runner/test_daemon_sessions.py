"""Daemon session lifecycle: runner sessions + machine sessions + poll + results.

Covers ``runners/<id>/sessions/`` (open/delete/poll),
``dev-machines/<id>/sessions/`` (open/delete/poll) and
``dev-machines/<id>/commands/<req>/result/``.

Poll notes: the first poll for a session replays the pending-entry list
without blocking; later polls block up to ``long_poll_interval_secs``. Tests
use a 40s client timeout and assert the envelope shape, not timing.
"""

from __future__ import annotations

import uuid

import pytest

from _harness.http import bearer_client

from .conftest import DAEMON

pytestmark = pytest.mark.contract


def _open_runner_session(client, runner_id: str, **body) -> dict:
    response = client.post(f"{DAEMON}/runners/{runner_id}/sessions/", json=body or {})
    assert response.status_code == 201, response.text
    return response.json()


def _post_poll(client, url: str, payload: dict, attempts: int = 3):
    """POST a long-poll, tolerating Django's async-Redis flake.

    The poll views share one module-global async Redis client across
    requests (``pi_dash/settings/redis.py``); under the dev server each
    request runs on a fresh event loop, so a poll can answer 500
    ``RuntimeError: Event loop is closed`` even though the contract
    response is 200. The retry only absorbs that transport-level 500 —
    the shape assertions below still pin the exact 200 envelope the Rust
    port must reproduce. Filed as a port-existing-bug note in the PR.
    """
    response = client.post(url, json=payload)
    for _ in range(attempts - 1):
        if response.status_code != 500:
            break
        response = client.post(url, json=payload)
    return response


# -- runner sessions -----------------------------------------------------

def test_daemon_runner_session_open_shape(settings, runner_client, enrolled):
    body = _open_runner_session(runner_client, enrolled["runner_id"])
    assert uuid.UUID(body["session_id"])
    welcome = body["welcome"]
    assert welcome["type"] == "welcome"
    assert welcome["rid"] == enrolled["runner_id"]
    assert welcome["server_time"]
    assert welcome["long_poll_interval_secs"] == 25
    assert welcome["protocol_version"] == 4
    assert "resume_ack" in body
    assert "redeliver" in body


def test_daemon_runner_session_open_rejects_cross_runner(settings, enrolled, seeder, daemon_world):
    other = seeder.enroll_runner(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        f"apd_en_{seeder.tag}xsession1x",
    )
    with bearer_client(settings.base_url, enrolled["access_token"]) as client:
        response = client.post(f"{DAEMON}/runners/{other['id']}/sessions/", json={})
    assert response.status_code == 401
    assert response.json() == {"detail": "runner_id_mismatch"}


def test_daemon_runner_session_open_requires_auth(anon_client, machine_flow):
    # No bearer token: auth returns None, so the view itself refuses with
    # 403 runner_id_mismatch (never 401 — nothing raised AuthenticationFailed).
    response = anon_client.post(f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/", json={})
    assert response.status_code == 403
    assert response.json() == {"error": "runner_id_mismatch"}


def test_daemon_runner_session_open_rejects_old_protocol(machine_client, machine_flow):
    response = machine_client.post(
        f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/",
        json={},
        headers={"X-Runner-Protocol-Version": "1"},
    )
    assert response.status_code == 426
    assert response.json()["error"] == "protocol_version_unsupported"


def test_daemon_runner_session_open_rejects_project_mismatch(machine_client, machine_flow):
    response = machine_client.post(
        f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/",
        json={"project_slug": "some-other-project"},
    )
    assert response.status_code == 409
    assert response.json()["error"] == "project_mismatch"
    assert response.json()["expected"]


def test_daemon_runner_session_delete_shape(machine_client, machine_flow):
    opened = _open_runner_session(machine_client, machine_flow["runner_id"])
    response = machine_client.delete(
        f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/{opened['session_id']}/"
    )
    assert response.status_code == 204
    # Idempotent: deleting an unknown session id is still 204.
    again = machine_client.delete(
        f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/{uuid.uuid4()}/"
    )
    assert again.status_code == 204


def test_daemon_runner_session_poll_shape(machine_client, machine_flow):
    client = machine_client
    opened = _open_runner_session(client, machine_flow["runner_id"])
    response = _post_poll(
        client,
        f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/{opened['session_id']}/poll",
        {"ack": [], "status": {"status": "idle"}},
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert isinstance(body["messages"], list)
    assert body["server_time"]
    assert body["long_poll_interval_secs"] == 25


def test_daemon_runner_session_poll_rejects_evicted(machine_client, machine_flow):
    client = machine_client
    first = _open_runner_session(client, machine_flow["runner_id"])
    _open_runner_session(client, machine_flow["runner_id"])
    response = client.post(
        f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/{first['session_id']}/poll",
        json={},
    )
    assert response.status_code == 409
    assert response.json()["error"] == "session_evicted"


def test_daemon_runner_session_poll_rejects_bad_json(machine_client, machine_flow):
    client = machine_client
    opened = _open_runner_session(client, machine_flow["runner_id"])
    response = client.post(
        f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/{opened['session_id']}/poll",
        content=b"{not json",
        headers={"Content-Type": "application/json"},
    )
    assert response.status_code == 400


def test_daemon_runner_session_poll_rejects_get(machine_client, machine_flow):
    client = machine_client
    opened = _open_runner_session(client, machine_flow["runner_id"])
    response = client.get(
        f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/{opened['session_id']}/poll"
    )
    assert response.status_code == 405


def test_daemon_runner_session_machine_token_wire_compat(settings, machine_flow):
    """Installed-daemon path: ``mt_`` bearer + URL runner identity, no JWT."""
    with bearer_client(settings.base_url, machine_flow["machine_token"], timeout=40.0) as client:
        opened = _open_runner_session(client, machine_flow["runner_id"])
        assert uuid.UUID(opened["session_id"])
        poll = _post_poll(
            client,
            f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/{opened['session_id']}/poll",
            {},
        )
        assert poll.status_code == 200
        assert isinstance(poll.json()["messages"], list)
        delete = client.delete(
            f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/{opened['session_id']}/"
        )
        assert delete.status_code == 204


# -- machine sessions ------------------------------------------------------

def _machine_ids(seeder, machine_flow) -> str:
    """Resolve the dev-machine id bound to the flow's machine token."""
    dev_machine_id = seeder.db.fetchval(
        "SELECT dev_machine_id FROM machine_token WHERE user_id = "
        "(SELECT owner_id FROM runner WHERE id = %s) AND host_label = %s",
        (machine_flow["runner_id"], machine_flow["host_label"]),
    )
    assert dev_machine_id, "machine token has no bound dev machine"
    return str(dev_machine_id)


def test_daemon_machine_session_lifecycle(settings, machine_flow, seeder):
    with bearer_client(settings.base_url, machine_flow["machine_token"], timeout=40.0) as client:
        dev_machine_id = _machine_ids(seeder, machine_flow)
        opened = client.post(f"{DAEMON}/dev-machines/{dev_machine_id}/sessions/", json={})
        assert opened.status_code == 201, opened.text
        body = opened.json()
        assert uuid.UUID(body["session_id"])
        assert body["welcome"]["type"] == "welcome"
        assert body["welcome"]["dev_machine_id"] == dev_machine_id
        assert body["welcome"]["long_poll_interval_secs"] == 25
        assert body["welcome"]["protocol_version"] == 4
        poll = _post_poll(
            client,
            f"{DAEMON}/dev-machines/{dev_machine_id}/sessions/{body['session_id']}/poll",
            {},
        )
        assert poll.status_code == 200, poll.text
        assert isinstance(poll.json()["messages"], list)
        delete = client.delete(
            f"{DAEMON}/dev-machines/{dev_machine_id}/sessions/{body['session_id']}/"
        )
        assert delete.status_code == 204
    seeder.db.execute(
        "DELETE FROM machine_session WHERE dev_machine_id = %s", (dev_machine_id,)
    )


def test_daemon_machine_session_rejects_cross_machine(settings, machine_flow, seeder, daemon_world):
    other = seeder.create_dev_machine(daemon_world["owner"]["id"])
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        response = client.post(f"{DAEMON}/dev-machines/{other['id']}/sessions/", json={})
    assert response.status_code == 403
    assert response.json() == {"error": "dev_machine_mismatch"}


def test_daemon_machine_session_requires_auth(settings, anon_client, machine_flow, seeder):
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        dev_machine_id = _machine_ids(seeder, machine_flow)
    # No bearer token: the machine-token auth returns None, so the open
    # view refuses with 403 dev_machine_mismatch.
    response = anon_client.post(f"{DAEMON}/dev-machines/{dev_machine_id}/sessions/", json={})
    assert response.status_code == 403
    assert response.json() == {"error": "dev_machine_mismatch"}


# -- machine command results -------------------------------------------------

def test_daemon_machine_command_result_unknown_request(settings, machine_flow, seeder):
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        dev_machine_id = _machine_ids(seeder, machine_flow)
        response = client.post(
            f"{DAEMON}/dev-machines/{dev_machine_id}/commands/{uuid.uuid4()}/result/",
            json={"status": "ok"},
        )
    # Unknown/expired request ids 404 without leaking which machines exist.
    assert response.status_code == 404
    assert response.json() == {"error": "unknown_request"}


def test_daemon_machine_command_result_rejects_cross_machine(
    settings, machine_flow, seeder, daemon_world
):
    other = seeder.create_dev_machine(daemon_world["owner"]["id"])
    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        response = client.post(
            f"{DAEMON}/dev-machines/{other['id']}/commands/{uuid.uuid4()}/result/",
            json={"status": "ok"},
        )
    assert response.status_code == 403
    assert response.json() == {"error": "dev_machine_mismatch"}
