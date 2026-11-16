"""Runner session-open consumer handoff on an empty PEL (PIDASHCONV-1485).

S1 opens and polls once (creating ``consumer-<S1>`` in the runner group),
then S2 opens and evicts S1. The session-open claim loop must delete the
evicted consumer even though the PEL is empty, so ``XINFO CONSUMERS`` lists
only ``consumer-<S2>`` afterwards.
"""

from __future__ import annotations

import pytest

from _harness.redis import client as redis_client

from .conftest import DAEMON

pytestmark = pytest.mark.contract


def _open_runner_session(client, runner_id: str) -> dict:
    response = client.post(f"{DAEMON}/runners/{runner_id}/sessions/", json={})
    assert response.status_code == 201, response.text
    return response.json()


def _poll_once(client, runner_id: str, session_id: str) -> None:
    # Tolerate Django's async-Redis 500 flake like test_daemon_sessions does.
    response = client.post(
        f"{DAEMON}/runners/{runner_id}/sessions/{session_id}/poll",
        json={"ack": [], "status": {"status": "idle"}},
    )
    for _ in range(2):
        if response.status_code != 500:
            break
        response = client.post(
            f"{DAEMON}/runners/{runner_id}/sessions/{session_id}/poll",
            json={"ack": [], "status": {"status": "idle"}},
        )
    assert response.status_code == 200, response.text


def _consumers(runner_id: str) -> list:
    rdb = redis_client()
    try:
        info = rdb.xinfo_consumers(
            f"runner_stream:{runner_id}", f"runner-group:{runner_id}"
        )
        return sorted(entry["name"] for entry in info)
    finally:
        rdb.close()


def test_session_open_deletes_evicted_consumer_on_empty_pel(
    machine_client, machine_flow
):
    rid = machine_flow["runner_id"]
    try:
        first = _open_runner_session(machine_client, rid)
        _poll_once(machine_client, rid, first["session_id"])
        assert _consumers(rid) == [f"consumer-{first['session_id']}"]

        second = _open_runner_session(machine_client, rid)
        assert _consumers(rid) == [f"consumer-{second['session_id']}"]
    finally:
        rdb = redis_client()
        try:
            rdb.delete(f"runner_stream:{rid}", f"runner_offline_stream:{rid}")
        finally:
            rdb.close()
