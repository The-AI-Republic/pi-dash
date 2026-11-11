# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Automatic engine pause clears pin releases + drains (PIDASHCONV-1732).

Black-box contract for the auto-pause dispatch hook: when a machine's
engine_auto_pause_reason flips automatically, Django runs
`on_engine_pause_changed` in the same transaction (release locality pins
on pause, drain pods; drain engine runners on resume). Rust had a placeholder
that only logged, so pins stayed and no drain fired until the 300s backstop.

Two tests:
- pause: seeded QUEUED locality-pinned run on the engine is unpinned
  (pinned_runner_id NULL) right after the auto flip, and the pod drains to
  the second online runner.
- resume: seeded QUEUED pod run with no other runner online is assigned to
  the engine when the pause clears via heartbeat available.
"""

from __future__ import annotations

import os
import secrets
import uuid

import pytest

from _harness import rust_queue
from _harness.celery import Broker, get_broker_url
from _harness.db import wait_for_condition
from _harness.http import bearer_client
from _harness.tokens import fingerprint, hash_token

from .conftest import DAEMON
from .test_daemon_sessions import _machine_ids, _post_poll
from .test_engine_credential import (
    _machine_engine_row,
    _mint_engine_credential,
)
from .test_engine_dispatch import (
    _engine_world,
    _go_online,
    _issue_in_backlog,
    _mk_runner,
    _queued_run,
    _run_row,
)

pytestmark = pytest.mark.contract

ENGINE = "/api/v1/runner/engine"
WEB = "/api/runners"
PAUSE_TASK = "runner.pause_engines_with_expired_credentials"
TIMEOUT = 60.0
SWEEP_BACKEND = os.environ.get("PIDASH_CONTRACT_BACKEND", "django")


@pytest.fixture()
def sweep_broker():
    if SWEEP_BACKEND == "rust":
        yield None
        return
    broker = Broker(get_broker_url())
    yield broker
    broker.close()


def _fire_sweep(broker):
    if SWEEP_BACKEND == "rust":
        celery_id = rust_queue.publish(PAUSE_TASK, [], {})
        rust_queue.wait_for_settled(celery_id, what=f"{PAUSE_TASK} settled")
        return
    broker.publish(PAUSE_TASK, [])


def _expire(machine_id, seeder):
    seeder.db.execute(
        "UPDATE engine_credential SET expires_at = now() - interval '1 minute'"
        " WHERE dev_machine_id = %s",
        (machine_id,),
    )
    seeder.db.execute(
        "UPDATE dev_machine SET engine_sign_in_state = 'active',"
        " engine_credential_expires_at = now() - interval '1 minute'"
        " WHERE id = %s",
        (machine_id,),
    )


def _open_machine_session(client, machine_id: str) -> str:
    response = client.post(f"{DAEMON}/dev-machines/{machine_id}/sessions/", json={})
    assert response.status_code == 201, response.text
    return response.json()["session_id"]


def _poll_machine(client, machine_id: str, session_id: str, payload: dict):
    return _post_poll(
        client,
        f"{DAEMON}/dev-machines/{machine_id}/sessions/{session_id}/poll",
        payload,
    )


def test_auto_pause_releases_locality_pins_and_drains_pod(
    seeder, daemon_world, sweep_broker
):
    """Expired-credential sweep unpins a QUEUED locality run immediately
    and drains the pod to the second runner (not after the 300s backstop)."""
    ws = daemon_world["workspace"]
    owner_id = daemon_world["owner"]["id"]
    pod = daemon_world["pod"]
    issue_id = _issue_in_backlog(seeder, daemon_world)
    # Engine runner (desktop_bundled, pod_member, online) + second runner online.
    engine_id, machine_id = _engine_world(seeder, daemon_world, pod_dispatch="pod_member")
    second_id = _mk_runner(
        seeder, owner_id, ws["id"], pod["id"], provisioning="manual", pod_dispatch="pod_member"
    )
    _go_online(seeder, second_id)
    # Pinned queued locality run on the engine, created just now.
    run_id = _queued_run(
        seeder,
        ws["id"],
        owner_id,
        pod["id"],
        issue_id,
        created_by=owner_id,
        pinned_runner_id=engine_id,
    )
    # Live credential, then expire it for the sweep.
    raw = "ec_" + secrets.token_urlsafe(32)
    _mint_engine_credential(seeder, owner_id, machine_id, raw)
    _expire(machine_id, seeder)

    _fire_sweep(sweep_broker)

    def pinned_cleared():
        row = seeder.db.fetchone(
            "SELECT pinned_runner_id FROM agent_run WHERE id = %s", (run_id,)
        )
        # Django does the release in the same tx as the pause flip.
        reason = seeder.db.fetchval(
            "SELECT engine_auto_pause_reason FROM dev_machine WHERE id = %s", (machine_id,)
        )
        if reason != "engine_sign_in_required":
            return None
        if row is None or row["pinned_runner_id"] is None:
            return row
        return None

    wait_for_condition(pinned_cleared, timeout=TIMEOUT, what="pin released after sweep")
    row = _run_row(seeder, run_id)
    assert row["pinned_runner_id"] is None, row

    def assigned_to_second():
        r = seeder.db.fetchone(
            "SELECT status, runner_id FROM agent_run WHERE id = %s", (run_id,)
        )
        if r and r["status"] == "assigned" and str(r["runner_id"]) == str(second_id):
            return r
        return None

    wait_for_condition(
        assigned_to_second, timeout=TIMEOUT, what="pod drain to second runner"
    )
    assert _machine_engine_row(seeder, machine_id)["engine_auto_pause_reason"] == "engine_sign_in_required"


def test_auto_resume_drains_to_engine_via_heartbeat(
    settings, seeder, daemon_world, machine_flow
):
    """Heartbeat available clears a wallet_empty pause and drains a QUEUED pod
    run to the engine (drain_for_runner_by_id)."""
    ws = daemon_world["workspace"]
    owner_id = daemon_world["owner"]["id"]
    pod = daemon_world["pod"]
    machine_id = _machine_ids(seeder, machine_flow)
    raw = "ec_" + secrets.token_urlsafe(32)
    _mint_engine_credential(seeder, owner_id, machine_id, raw)
    # Create engine runner in same pod as daemon world, pod_member, online.
    # Use daemon_world pod so the run lands there; the machine is the flow machine
    # which shares the same pod? In this suite the flow machine's runner is in
    # daemon_world pod implicitly? Instead seed a fresh engine in the daemon pod.
    engine_id, _ = _engine_world(seeder, daemon_world, pod_dispatch="pod_member")
    # The engine is bound to a different machine than machine_flow's machine;
    # for the heartbeat resume path the machine is machine_flow's machine, so we
    # need the engine runner to belong to that machine. Create it manually.
    # _engine_world already created a machine; we need to re-point its engine
    # runner's dev_machine_id to the flow machine for this test, or create a new one.
    # Simpler: point the flow machine's existing runner to pod_member and online,
    # and use it as the engine. Instead we reuse machine_flow runner and promote it.
    # Promote machine_flow runner to desktop_bundled engine in daemon pod.
    seeder.db.execute(
        "UPDATE runner SET provisioning = 'desktop_bundled', pod_dispatch = 'pod_member',"
        " pod_id = %s, dev_machine_id = %s WHERE id = %s",
        (pod["id"], machine_id, machine_flow["runner_id"]),
    )
    _go_online(seeder, machine_flow["runner_id"])
    engine_runner = machine_flow["runner_id"]

    issue_id = _issue_in_backlog(seeder, daemon_world)
    run_id = _queued_run(
        seeder,
        ws["id"],
        owner_id,
        pod["id"],
        issue_id,
        created_by=owner_id,
    )
    # Seed auto-paused state.
    seeder.db.execute(
        "UPDATE dev_machine SET engine_auto_pause_reason = 'wallet_empty',"
        " engine_sign_in_state = 'active' WHERE id = %s",
        (machine_id,),
    )
    # Ensure no other online runner in the pod except the engine.
    seeder.db.execute(
        "UPDATE runner SET status = 'offline', last_heartbeat_at = NULL"
        " WHERE pod_id = %s AND id != %s",
        (pod["id"], engine_runner),
    )
    # Runner session for drain delivery (engine needs live session)
    drain_session_id = str(uuid.uuid4())
    seeder.db.execute(
        "INSERT INTO runner_session (id, runner_id, protocol_version,"
        " created_at, last_seen_at, revoked_at, revoked_reason)"
        " VALUES (%s,%s,4,now(),now(),NULL,'')",
        (drain_session_id, engine_runner),
    )
    seeder._put("runner_session", drain_session_id)

    with bearer_client(settings.base_url, machine_flow["machine_token"]) as client:
        session_id = _open_machine_session(client, machine_id)
        response = _poll_machine(
            client,
            machine_id,
            session_id,
            {"engine_state": {"state": "available"}},
        )
        assert response.status_code == 200, response.text

    assert _machine_engine_row(seeder, machine_id)["engine_auto_pause_reason"] == ""

    def assigned_to_engine():
        r = seeder.db.fetchone(
            "SELECT status, runner_id FROM agent_run WHERE id = %s", (run_id,)
        )
        if r and r["status"] == "assigned" and str(r["runner_id"]) == str(engine_runner):
            return r
        return None

    wait_for_condition(assigned_to_engine, timeout=TIMEOUT, what="engine drain after heartbeat available")
