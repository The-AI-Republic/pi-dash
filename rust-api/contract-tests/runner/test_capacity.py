"""Capacity contract: POST /runs/<id>/capacity/ waiting/admit/defer/reconcile.

New-system-only (DIFF): Django has no capacity endpoint (404); Rust implements the full
contract. Tests seed an ASSIGNED run with protocol-1 attempt identity and verify the
typed decisions, token/session fencing, and validation.
"""

from __future__ import annotations

import uuid

import pytest

from .conftest import DAEMON

pytestmark = pytest.mark.contract

def _idem():
    return {"Idempotency-Key": str(uuid.uuid4())}

def _open_session(machine_client, runner_id, **extra):
    body = {"capabilities": ["machine_run_queue_v1"], **extra}
    resp = machine_client.post(f"{DAEMON}/runners/{runner_id}/sessions/", json=body)
    assert resp.status_code == 201, resp.text
    return resp.json()["session_id"]

def _seed_protocol1(seeder, run_id):
    token = str(uuid.uuid4())
    seeder.db.execute(
        "UPDATE agent_run SET assignment_token=%s, assignment_protocol=1, capacity_wait=NULL WHERE id=%s",
        (token, run_id),
    )
    return token

def test_capacity_waiting_recorded(machine_client, machine_flow, daemon_run, seeder):
    token = _seed_protocol1(seeder, daemon_run["id"])
    sess = _open_session(machine_client, machine_flow["runner_id"])
    resp = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/capacity/",
        json={"action": "waiting", "session_id": sess, "assignment_token": token, "revision": 1},
        headers=_idem(),
    )
    assert resp.status_code == 200, resp.text
    data = resp.json()
    assert data["decision"] == "recorded"
    assert data["assignment_protocol"] == 1
    assert data["assignment_token"] == token
    # capacity_wait stored
    row = seeder.db.fetchone("SELECT capacity_wait FROM agent_run WHERE id=%s", (daemon_run["id"],))
    assert row["capacity_wait"]["phase"] == "waiting_for_capacity"

def test_capacity_admit_proceed(machine_client, machine_flow, daemon_run, seeder):
    token = _seed_protocol1(seeder, daemon_run["id"])
    sess = _open_session(machine_client, machine_flow["runner_id"])
    key = str(uuid.uuid4())
    resp = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/capacity/",
        json={"action": "admit", "session_id": sess, "assignment_token": token, "revision": 1},
        headers={"Idempotency-Key": key},
    )
    assert resp.status_code == 200, resp.text
    data = resp.json()
    assert data["decision"] == "proceed"
    assert data["admission_id"] == key

def test_capacity_validation_errors(machine_client, machine_flow, daemon_run):
    sess = _open_session(machine_client, machine_flow["runner_id"])
    # missing session_id
    resp = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/capacity/",
        json={"action": "waiting", "revision": 1},
        headers=_idem(),
    )
    assert resp.status_code == 400
    assert resp.json()["error"] == "session_id_required"
    # invalid action
    resp = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/capacity/",
        json={"action": "bogus", "session_id": sess, "revision": 1},
        headers=_idem(),
    )
    assert resp.status_code == 400
    assert resp.json()["error"] == "invalid_action"

def test_capacity_stale_assignment(machine_client, machine_flow, daemon_run, seeder):
    token = _seed_protocol1(seeder, daemon_run["id"])
    wrong = str(uuid.uuid4())
    sess = _open_session(machine_client, machine_flow["runner_id"])
    resp = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/capacity/",
        json={"action": "waiting", "session_id": sess, "assignment_token": wrong, "revision": 1},
        headers=_idem(),
    )
    assert resp.status_code == 200, resp.text
    assert resp.json()["decision"] == "stale_assignment"

def test_capacity_defer_flow(machine_client, machine_flow, daemon_run, seeder):
    token = _seed_protocol1(seeder, daemon_run["id"])
    sess = _open_session(machine_client, machine_flow["runner_id"])
    key = str(uuid.uuid4())
    admit = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/capacity/",
        json={"action": "admit", "session_id": sess, "assignment_token": token, "revision": 1},
        headers={"Idempotency-Key": key},
    )
    assert admit.status_code == 200, admit.text
    assert admit.json()["decision"] == "proceed"
    # defer requires admission_id and higher revision
    defer = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/capacity/",
        json={"action": "defer", "session_id": sess, "assignment_token": token, "revision": 2, "admission_id": key},
        headers=_idem(),
    )
    assert defer.status_code == 200, defer.text
    assert defer.json()["decision"] == "recorded"
    row = seeder.db.fetchone("SELECT capacity_wait FROM agent_run WHERE id=%s", (daemon_run["id"],))
    assert row["capacity_wait"]["phase"] == "waiting_for_capacity"

def test_capacity_admit_idempotent_same_key(machine_client, machine_flow, daemon_run, seeder):
    token = _seed_protocol1(seeder, daemon_run["id"])
    sess = _open_session(machine_client, machine_flow["runner_id"])
    key = str(uuid.uuid4())
    r1 = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/capacity/",
        json={"action": "admit", "session_id": sess, "assignment_token": token, "revision": 1},
        headers={"Idempotency-Key": key},
    )
    r2 = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/capacity/",
        json={"action": "admit", "session_id": sess, "assignment_token": token, "revision": 1},
        headers={"Idempotency-Key": key},
    )
    assert r1.status_code == 200 and r2.status_code == 200
    assert r1.json()["decision"] == "proceed"
    assert r2.json()["decision"] == "proceed"
    assert r2.json()["admission_id"] == key

def test_capacity_reconcile_retains_protocol1(machine_client, machine_flow, daemon_run, seeder):
    token = _seed_protocol1(seeder, daemon_run["id"])
    sess = _open_session(machine_client, machine_flow["runner_id"])
    resp = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/capacity/",
        json={"action": "reconcile", "session_id": sess, "assignment_token": token, "revision": 1},
        headers=_idem(),
    )
    assert resp.status_code == 200, resp.text
    assert resp.json()["decision"] == "retain"
    row = seeder.db.fetchone("SELECT assignment_protocol, assignment_token FROM agent_run WHERE id=%s", (daemon_run["id"],))
    assert row["assignment_protocol"] == 1
    assert str(row["assignment_token"]) == token

def test_capacity_legacy_adoption(machine_client, machine_flow, daemon_run, seeder):
    # Legacy run (protocol 0, no token): reconcile adopts with fresh token.
    seeder.db.execute(
        "UPDATE agent_run SET assignment_token=NULL, assignment_protocol=0, capacity_wait=NULL WHERE id=%s",
        (daemon_run["id"],),
    )
    sess = _open_session(machine_client, machine_flow["runner_id"])
    # Non-reconcile on legacy stays legacy_in_flight.
    r1 = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/capacity/",
        json={"action": "waiting", "session_id": sess, "revision": 1},
        headers=_idem(),
    )
    assert r1.status_code == 200, r1.text
    assert r1.json()["decision"] == "legacy_in_flight"
    # Reconcile adopts.
    r2 = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/capacity/",
        json={"action": "reconcile", "session_id": sess, "revision": 1},
        headers=_idem(),
    )
    assert r2.status_code == 200, r2.text
    assert r2.json()["decision"] == "retain"
    row = seeder.db.fetchone("SELECT assignment_protocol, assignment_token FROM agent_run WHERE id=%s", (daemon_run["id"],))
    assert row["assignment_protocol"] == 1
    assert row["assignment_token"] is not None

def test_capacity_terminal_decision(machine_client, machine_flow, daemon_run, seeder):
    token = _seed_protocol1(seeder, daemon_run["id"])
    seeder.db.execute("UPDATE agent_run SET status='completed' WHERE id=%s", (daemon_run["id"],))
    sess = _open_session(machine_client, machine_flow["runner_id"])
    resp = machine_client.post(
        f"{DAEMON}/runs/{daemon_run['id']}/capacity/",
        json={"action": "waiting", "session_id": sess, "assignment_token": token, "revision": 1},
        headers=_idem(),
    )
    assert resp.status_code == 200, resp.text
    assert resp.json()["decision"] == "terminal"
