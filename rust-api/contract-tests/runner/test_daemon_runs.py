"""Daemon run lifecycle: ``runs/<id>/accept|queued|started|events|approvals|...``.

Every endpoint is runner-scoped (``_resolve`` → 404 unknown run, 403 foreign
run) and idempotency-aware (``Idempotency-Key`` → ``{ok: true, duplicate}``).
Terminal transitions each get a fresh seeded run — one run exercises exactly
one terminal path.
"""

from __future__ import annotations

import uuid

import pytest

from _harness.http import bearer_client

from .conftest import DAEMON

pytestmark = pytest.mark.contract


def _idem() -> dict:
    return {"Idempotency-Key": uuid.uuid4().hex}


def test_daemon_run_accept_shape(machine_client, daemon_run, seeder):
    response = machine_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    assert response.status_code == 200, response.text
    assert response.json() == {"ok": True}
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "running"
    )


def test_daemon_run_endpoints_dedupe(machine_client, daemon_run):
    key = _idem()
    first = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={}, headers=key
    )
    assert first.json() == {"ok": True}
    second = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={}, headers=key
    )
    assert second.json() == {"ok": True, "duplicate": True}


def test_daemon_run_queued_is_ack_only(machine_client, daemon_run, seeder):
    response = machine_client.post(f"{DAEMON}/runs/{daemon_run['id']}/queued/", json={})
    assert response.json() == {"ok": True, "ignored": True}
    # The retired worktree queue performs no transition: still assigned.
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "assigned"
    )


def test_daemon_run_started_shape(machine_client, daemon_run, seeder):
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/started/",
        json={"thread_id": "thr-1", "model": "test-model"},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    row = seeder.db.fetchone(
        "SELECT status, thread_id, llm_model FROM agent_run WHERE id = %s", (daemon_run["id"],)
    )
    assert row["status"] == "running"
    assert row["thread_id"] == "thr-1"
    assert row["llm_model"] == "test-model"


def test_daemon_run_events_shape(machine_client, daemon_run, seeder):
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/events/",
        json={"events": [{"seq": 1, "kind": "log", "payload": {"text": "hi"}}]},
        headers=_idem(),
    )
    assert response.json() == {"ok": True, "accepted": 1}
    assert (
        seeder.db.fetchval(
            "SELECT COUNT(*) FROM agent_run_event WHERE agent_run_id = %s", (daemon_run["id"],)
        )
        == 1
    )


def test_daemon_run_approvals_shape(machine_client, daemon_run, seeder):
    approval_id = str(uuid.uuid4())
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/approvals/",
        json={"approval_id": approval_id, "kind": "command_execution", "reason": "rm -rf /"},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    row = seeder.db.fetchone(
        "SELECT status FROM agent_run_approval WHERE id = %s", (approval_id,)
    )
    assert row["status"] == "pending"
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "awaiting_approval"
    )


def test_daemon_run_awaiting_reauth_shape(machine_client, daemon_run, seeder):
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/awaiting-reauth/", json={}, headers=_idem()
    )
    assert response.json() == {"ok": True}
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "awaiting_reauth"
    )


def test_daemon_run_complete_shape(machine_client, daemon_run, seeder):
    machine_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/complete/",
        json={"done_payload": {"summary": "done"}},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "completed"
    )


def test_daemon_run_pause_shape(machine_client, daemon_run):
    machine_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/pause/",
        json={"payload": {"question": "proceed?"}},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}


def test_daemon_run_fail_shape(machine_client, daemon_run, seeder):
    machine_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/fail/",
        json={"detail": "boom"},
        headers=_idem(),
    )
    assert response.json() == {"ok": True}
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "failed"
    )


def test_daemon_run_fail_refusal_shape(machine_client, daemon_run, seeder):
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/fail/",
        json={"reason": "refusal", "category": "cyber", "detail": "declined"},
        headers=_idem(),
    )
    assert response.json() == {"ok": True, "refused": True}
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "refused"
    )


def test_daemon_run_cancelled_shape(machine_client, daemon_run, seeder):
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/cancelled/", json={}, headers=_idem()
    )
    assert response.json() == {"ok": True}
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "cancelled"
    )


def test_daemon_run_resumed_shape(machine_client, daemon_run, seeder):
    machine_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/resumed/", json={}, headers=_idem()
    )
    assert response.json() == {"ok": True}
    assert (
        seeder.db.fetchval("SELECT status FROM agent_run WHERE id = %s", (daemon_run["id"],))
        == "running"
    )


def test_daemon_run_stream_upgrade_shape(machine_client, daemon_run):
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/stream/upgrade/", json={"stream": "events"}
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["ticket"]
    assert body["expires_in_secs"] == 60


def test_daemon_run_stream_upgrade_rejects_stream(machine_client, daemon_run):
    response = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/stream/upgrade/", json={"stream": "video"}
    )
    assert response.status_code == 400
    assert response.json() == {"error": "invalid_stream"}


def test_daemon_run_not_found(machine_client):
    response = machine_client.post(f"{DAEMON}/runs/{uuid.uuid4()}/accept/", json={})
    assert response.status_code == 404
    assert response.json() == {"error": "run_not_found"}


def test_daemon_run_rejects_foreign_runner(settings, seeder, daemon_world, daemon_run):
    """Tenant isolation: a run owned by another runner (other workspace) is 403."""
    other_owner = seeder.create_user()
    other_ws = seeder.create_workspace(other_owner["id"])
    seeder.create_member(other_ws["id"], other_owner["id"])
    other_project = seeder.create_project(other_ws["id"])
    other_pod = seeder.create_pod(other_ws["id"], other_project["id"], is_default=True)
    other_runner = seeder.enroll_runner(
        other_owner["id"], other_ws["id"], other_pod["id"], f"apd_en_{seeder.tag}foreign1"
    )
    # Enroll the foreign runner to get a real access token for it.
    import httpx

    with httpx.Client(base_url=settings.base_url, timeout=30.0) as anon:
        enroll = anon.post(
            f"{DAEMON}/runners/enroll/",
            json={"enrollment_token": other_runner["enrollment_token"], "host_label": "foreign-h"},
        )
        assert enroll.status_code == 201, enroll.text
        foreign_token = enroll.json()["access_token"]
    with bearer_client(settings.base_url, foreign_token) as foreign_client:
        response = foreign_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    assert response.status_code == 403
    assert response.json() == {"error": "run_not_owned_by_runner"}
    # Teardown rows the foreign enroll minted.
    seeder.db.execute(
        "DELETE FROM machine_token WHERE user_id = %s AND host_label = 'foreign-h'",
        (other_owner["id"],),
    )
    seeder.db.execute(
        "UPDATE runner SET dev_machine_id = NULL WHERE id = %s", (other_runner["id"],)
    )
    seeder.db.execute(
        "DELETE FROM dev_machine WHERE owner_id = %s AND host_label = 'foreign-h'",
        (other_owner["id"],),
    )


def test_daemon_run_requires_auth(anon_client, daemon_run):
    # No bearer token: auth returns None and the run resolves as not-owned.
    response = anon_client.post(f"{DAEMON}/runs/{daemon_run['id']}/accept/", json={})
    assert response.status_code == 403
    assert response.json() == {"error": "run_not_owned_by_runner"}
