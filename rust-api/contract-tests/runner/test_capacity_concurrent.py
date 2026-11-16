"""Concurrent capacity: open/admit/poll racing must serialize, not convoy.

NEWPIDASH-241 D7: the capacity endpoint takes Runner → Session → Run locks
with bounded waits (5s lock / 20s statement). Competing admits, polls and
session opens on the same attempt must all complete with typed decisions
(never 500/503 deadlock) and leave a consistent stored state.
"""

from __future__ import annotations

import threading
import uuid

import pytest

from _harness.http import bearer_client
from .conftest import DAEMON

pytestmark = pytest.mark.contract


def _seed_protocol1(seeder, run_id):
    token = str(uuid.uuid4())
    seeder.db.execute(
        "UPDATE agent_run SET assignment_token=%s, assignment_protocol=1, capacity_wait=NULL WHERE id=%s",
        (token, run_id),
    )
    return token


def _open_session(client, runner_id):
    resp = client.post(
        f"{DAEMON}/runners/{runner_id}/sessions/",
        json={"capabilities": ["machine_run_queue_v1"]},
    )
    assert resp.status_code == 201, resp.text
    return resp.json()["session_id"]


def _client_for(settings, machine_flow):
    return bearer_client(
        settings.base_url,
        machine_flow["machine_token"],
        headers={"X-Runner-Id": machine_flow["runner_id"]},
    )


def test_capacity_concurrent_admits_serialize(settings, machine_flow, daemon_run, seeder):
    token = _seed_protocol1(seeder, daemon_run["id"])
    with _client_for(settings, machine_flow) as client:
        sess = _open_session(client, machine_flow["runner_id"])
        results = []

        def admit():
            with _client_for(settings, machine_flow) as c:
                resp = c.post(
                    f"{DAEMON}/runs/{daemon_run['id']}/capacity/",
                    json={
                        "action": "admit",
                        "session_id": sess,
                        "assignment_token": token,
                        "revision": 1,
                    },
                    headers={"Idempotency-Key": str(uuid.uuid4())},
                )
                results.append((resp.status_code, resp.json().get("decision")))

        threads = [threading.Thread(target=admit) for _ in range(5)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=20)
        assert all(not t.is_alive() for t in threads), "admits deadlocked"
        assert len(results) == 5
        # No convoy: every admit is a typed 200, never 500/503.
        for code, decision in results:
            assert code == 200, (code, decision)
            assert decision in ("proceed", "stale_report", "recorded")
        # Exactly one winner claims preparing; the rest see stale_report.
        decisions = [d for _c, d in results]
        assert decisions.count("proceed") == 1, decisions
        row = seeder.db.fetchone(
            "SELECT capacity_wait FROM agent_run WHERE id=%s", (daemon_run["id"],)
        )
        assert row["capacity_wait"]["phase"] == "preparing"


def test_capacity_concurrent_waiting_poll_and_open(settings, machine_flow, daemon_run, seeder):
    token = _seed_protocol1(seeder, daemon_run["id"])
    with _client_for(settings, machine_flow) as client:
        sess = _open_session(client, machine_flow["runner_id"])
        outcomes = {}

        def waiting():
            with _client_for(settings, machine_flow) as c:
                resp = c.post(
                    f"{DAEMON}/runs/{daemon_run['id']}/capacity/",
                    json={
                        "action": "waiting",
                        "session_id": sess,
                        "assignment_token": token,
                        "revision": 1,
                    },
                    headers={"Idempotency-Key": str(uuid.uuid4())},
                )
                outcomes["waiting"] = (resp.status_code, resp.json().get("decision"))

        def poll():
            with _client_for(settings, machine_flow) as c:
                resp = c.post(
                    f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/{sess}/poll",
                    json={"ack": [], "status": {"status": "idle"}},
                )
                outcomes["poll"] = resp.status_code

        def open_new():
            with _client_for(settings, machine_flow) as c:
                resp = c.post(
                    f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/",
                    json={"capabilities": ["machine_run_queue_v1"]},
                )
                outcomes["open"] = resp.status_code

        threads = [
            threading.Thread(target=waiting),
            threading.Thread(target=poll),
            threading.Thread(target=open_new),
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=20)
        assert all(not t.is_alive() for t in threads), "concurrent ops deadlocked"
        # Bounded waits absorb the convoy: no 503 runner_state_locked.
        assert outcomes["waiting"] == (200, "recorded"), outcomes
        assert outcomes["poll"] == 200, outcomes
        assert outcomes["open"] in (201, 409), outcomes


def test_capacity_concurrent_cancel_decision(settings, machine_flow, daemon_run, seeder):
    token = _seed_protocol1(seeder, daemon_run["id"])
    seeder.db.execute(
        "UPDATE agent_run SET status='cancel_requested' WHERE id=%s", (daemon_run["id"],)
    )
    with _client_for(settings, machine_flow) as client:
        sess = _open_session(client, machine_flow["runner_id"])
        decisions = []

        def wait():
            with _client_for(settings, machine_flow) as c:
                resp = c.post(
                    f"{DAEMON}/runs/{daemon_run['id']}/capacity/",
                    json={
                        "action": "waiting",
                        "session_id": sess,
                        "assignment_token": token,
                        "revision": 1,
                    },
                    headers={"Idempotency-Key": str(uuid.uuid4())},
                )
                decisions.append((resp.status_code, resp.json().get("decision")))

        threads = [threading.Thread(target=wait) for _ in range(3)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=20)
        assert all(not t.is_alive() for t in threads)
        assert len(decisions) == 3
        for code, decision in decisions:
            assert code == 200, (code, decision)
            assert decision == "cancel", (code, decision)
