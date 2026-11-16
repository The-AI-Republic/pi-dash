"""Runner / dev-machine long-poll: Redis XREADGROUP budget + mid-poll delivery.

PIDASHCONV-1209. The runner and dev-machine long-poll handlers slice a
blocking ``XREADGROUP`` into ``<= 1s`` blocks (``_POLL_SLICE_MS``). Django
builds its Redis client with ``socket_timeout = REDIS_SOCKET_TIMEOUT`` (5s),
so a 1s block completes server-side and nil comes back cleanly. A client
whose response timeout is *shorter* than the block (redis 1.7.1's default is
500ms) has every blocking read cut client-side: the read is abandoned, an
empty result is returned, and the loop reconnects and re-issues. An entry
Redis hands to the consumer after the cut but before the connection is torn
down lands in the consumer's PEL and its reply is dropped.

Fingerprint: Redis increments ``cmdstat_xreadgroup`` only for a blocking read
that *completes* — a read cut by a client disconnect never counts (verified:
``XREADGROUP BLOCK 3000`` killed at 0.6s leaves ``calls`` unchanged, while a
``BLOCK 1200`` that returns nil twice bumps it by one). So a correct backend
completes about one read per 1s slice over a window, while a backend whose
client response timeout undercuts the slice completes none of the blocking
reads (only the initial non-blocking PEL-replay read). The delta over one
idle long-poll is that count. The counter is server-global, so run this
suite against a dedicated Redis instance.

A second test proves delivery: an entry ``XADD``-ed while a poll is blocked
comes back in that same poll's response.
"""

from __future__ import annotations

import json
import threading
import time
import uuid

import pytest

from _harness.http import bearer_client
from _harness.redis import redis_url

from .conftest import DAEMON

pytestmark = pytest.mark.contract

_POLL_TIMEOUT = 60.0


def _redis():
    import redis

    return redis.Redis.from_url(redis_url(), decode_responses=True)


def _completed_xreadgroups(rdb) -> int:
    stats = rdb.info("commandstats")
    entry = stats.get("cmdstat_xreadgroup")
    return int(entry["calls"]) if entry else 0


def _open_runner_session(client, runner_id: str) -> dict:
    response = client.post(f"{DAEMON}/runners/{runner_id}/sessions/", json={})
    assert response.status_code == 201, response.text
    return response.json()


def _dev_machine_id(seeder, machine_flow) -> str:
    dev_machine_id = seeder.db.fetchval(
        "SELECT dev_machine_id FROM machine_token WHERE user_id = "
        "(SELECT owner_id FROM runner WHERE id = %s) AND host_label = %s",
        (machine_flow["runner_id"], machine_flow["host_label"]),
    )
    assert dev_machine_id, "machine token has no bound dev machine"
    return str(dev_machine_id)


def _assert_one_completed_read_per_slice(delta: int, window: int, subject: str) -> None:
    # Each 1s slice that completes blocks server-side and returns nil, so a
    # healthy backend completes about `window` reads. The first poll (PEL
    # replay) is drained before measuring, so a delta near 1 means the
    # blocking reads are being cut client-side instead of completing.
    # A lower bound only: concurrent sessions on the same Redis can only add
    # completed reads, never remove them.
    floor = max(1, window - 2)
    assert delta >= floor, (
        f"idle {subject} long-poll completed only {delta} XREADGROUP reads "
        f"over a {window}s window (expected ~{window}); a client response "
        f"timeout shorter than the 1s slice abandons every blocking read, so "
        f"none of them complete"
    )


def test_runner_poll_xreadgroup_budget(settings, machine_flow):
    """One idle runner long-poll completes about one XREADGROUP per 1s slice."""
    rid = machine_flow["runner_id"]
    with bearer_client(
        settings.base_url, machine_flow["machine_token"], timeout=_POLL_TIMEOUT
    ) as client:
        opened = _open_runner_session(client, rid)
        poll_url = f"{DAEMON}/runners/{rid}/sessions/{opened['session_id']}/poll"
        # First poll drains the PEL replay (STREAMS 0, non-blocking) so the
        # measured poll blocks with STREAMS >.
        drain = client.post(poll_url, json={"ack": [], "status": {"status": "idle"}})
        assert drain.status_code == 200, drain.text

        rdb = _redis()
        before = _completed_xreadgroups(rdb)
        response = client.post(poll_url, json={"ack": [], "status": {"status": "idle"}})
        after = _completed_xreadgroups(rdb)
        assert response.status_code == 200, response.text
        window = response.json()["long_poll_interval_secs"]
        assert window >= 1

        _assert_one_completed_read_per_slice(after - before, window, "runner")


def test_machine_poll_xreadgroup_budget(settings, machine_flow, seeder):
    """Same completed-read budget for the dev-machine long-poll handler."""
    with bearer_client(
        settings.base_url, machine_flow["machine_token"], timeout=_POLL_TIMEOUT
    ) as client:
        dev_machine_id = _dev_machine_id(seeder, machine_flow)
        opened = client.post(f"{DAEMON}/dev-machines/{dev_machine_id}/sessions/", json={})
        assert opened.status_code == 201, opened.text
        sid = opened.json()["session_id"]
        poll_url = f"{DAEMON}/dev-machines/{dev_machine_id}/sessions/{sid}/poll"
        drain = client.post(poll_url, json={})
        assert drain.status_code == 200, drain.text

        rdb = _redis()
        before = _completed_xreadgroups(rdb)
        response = client.post(poll_url, json={})
        after = _completed_xreadgroups(rdb)
        assert response.status_code == 200, response.text
        window = response.json()["long_poll_interval_secs"]

        _assert_one_completed_read_per_slice(after - before, window, "dev-machine")
        seeder.db.execute(
            "DELETE FROM machine_session WHERE dev_machine_id = %s", (dev_machine_id,)
        )


def test_runner_poll_delivers_midpoll_entry(settings, machine_flow):
    """An entry XADD-ed while the poll is blocked returns in that poll."""
    rid = machine_flow["runner_id"]
    with bearer_client(
        settings.base_url, machine_flow["machine_token"], timeout=_POLL_TIMEOUT
    ) as client:
        opened = _open_runner_session(client, rid)
        poll_url = f"{DAEMON}/runners/{rid}/sessions/{opened['session_id']}/poll"
        drain = client.post(poll_url, json={"ack": [], "status": {"status": "idle"}})
        assert drain.status_code == 200, drain.text

        rdb = _redis()
        result: dict = {}

        def _run_poll():
            result["response"] = client.post(
                poll_url, json={"ack": [], "status": {"status": "idle"}}
            )

        thread = threading.Thread(target=_run_poll)
        thread.start()
        try:
            # Let the poll enter its blocking XREADGROUP before injecting.
            time.sleep(1.5)
            mid = str(uuid.uuid4())
            payload = {"mid": mid, "type": "assign", "run_id": str(uuid.uuid4())}
            rdb.xadd(
                f"runner_stream:{rid}",
                {"mid": mid, "type": "assign", "payload": json.dumps(payload)},
            )
        finally:
            thread.join(timeout=_POLL_TIMEOUT * 2)

        response = result["response"]
        assert response.status_code == 200, response.text
        messages = response.json()["messages"]
        assert any(m["type"] == "assign" and m["mid"] == mid for m in messages), (
            f"mid-poll assign entry not delivered in the poll response: {messages!r}"
        )
