"""Offline-runner recovery: poll-path cancel reasons (PIDASHCONV-1063).

Ports the poll half of Django's
``tests/unit/runner/test_offline_runner_recovery.py`` (``70d684af``):
a daemon that returns with an in-flight run the offline sweep
finalized is told to stop with ``run_already_<status>``; a
still-pending cancellation keeps ``cancellation_pending``; anything
else stays quiet. The sweep bodies themselves are task-oracle cases
in ``test_offline_runner_sweeps.py`` (worker-plane, not the matrix).

No worker runs here: the tests seed the post-sweep row state
directly and drive the session long-poll.

Assertion level: the cancel frame enqueued to the runner's Redis
control stream (``runner_stream:<rid>``), not the poll response's
``messages`` list. The retry is queued synchronously during poll
bookkeeping, but the first poll replays only the session PEL while
follow-up (blocking) polls hit Django's async-Redis "Event loop is
closed" flake under runserver — so delivery-to-response is timing
sensitive, while enqueue-to-stream is deterministic on both
backends (the Rust port writes the same streams).
"""

from __future__ import annotations

import json
import os

import pytest
import redis

from .conftest import DAEMON
from .test_daemon_sessions import _open_runner_session, _post_poll

pytestmark = pytest.mark.contract

RUNNER_OFFLINE = "runner_offline"


def _stream(redis_url=None):
    url = redis_url or os.environ.get("REDIS_URL")
    assert url, "REDIS_URL is not set (runner control stream)"
    return redis.Redis.from_url(url, decode_responses=True)


def _cancel_frames(client, runner_id):
    """Cancel frames currently in the runner's control stream."""
    frames = []
    for _entry_id, fields in client.xrange(f"runner_stream:{runner_id}"):
        if fields.get("type") != "cancel":
            continue
        try:
            frames.append(json.loads(fields["payload"]))
        except (KeyError, ValueError):
            continue
    return frames


def _seed_claim(seeder, daemon_world, machine_flow, *, status, error_code=""):
    """A run row bound to the flow runner, shaped like a sweep outcome."""
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status=status,
    )
    bag = {"message": "seeded claim"}
    if error_code:
        bag["code"] = error_code
    seeder.db.execute(
        "UPDATE agent_run SET error_details = %s::jsonb WHERE id = %s",
        (json.dumps(bag), run["id"]),
    )
    return run


def _poll_once(machine_client, machine_flow, session_id, run_id):
    """One non-blocking poll with an in-flight run; returns the stream client."""
    url = f"{DAEMON}/runners/{machine_flow['runner_id']}/sessions/{session_id}/poll"
    response = _post_poll(
        machine_client,
        url,
        {"ack": [], "status": {"status": "busy", "in_flight_run": run_id}},
    )
    assert response.status_code == 200, response.text
    return _stream()


def test_poll_failed_offline_run_cancels_with_run_already(
    machine_client, machine_flow, daemon_world, seeder
):
    run = _seed_claim(
        seeder, daemon_world, machine_flow, status="failed", error_code=RUNNER_OFFLINE
    )
    opened = _open_runner_session(machine_client, machine_flow["runner_id"])
    stream = _poll_once(machine_client, machine_flow, opened["session_id"], run["id"])
    cancels = [
        frame
        for frame in _cancel_frames(stream, machine_flow["runner_id"])
        if frame.get("run_id") == run["id"]
    ]
    assert cancels, f"no cancel frame for {run['id']}"
    assert cancels[0]["reason"] == "run_already_failed"


def test_poll_cancels_stamped_cancelled_run_with_run_already(
    machine_client, machine_flow, daemon_world, seeder
):
    run = _seed_claim(
        seeder, daemon_world, machine_flow, status="cancelled", error_code=RUNNER_OFFLINE
    )
    opened = _open_runner_session(machine_client, machine_flow["runner_id"])
    stream = _poll_once(machine_client, machine_flow, opened["session_id"], run["id"])
    cancels = [
        frame
        for frame in _cancel_frames(stream, machine_flow["runner_id"])
        if frame.get("run_id") == run["id"]
    ]
    assert cancels, f"no cancel frame for {run['id']}"
    assert cancels[0]["reason"] == "run_already_cancelled"


def test_poll_pending_cancellation_keeps_old_reason(
    machine_client, machine_flow, daemon_world, seeder
):
    run = _seed_claim(seeder, daemon_world, machine_flow, status="cancel_requested")
    opened = _open_runner_session(machine_client, machine_flow["runner_id"])
    stream = _poll_once(machine_client, machine_flow, opened["session_id"], run["id"])
    cancels = [
        frame
        for frame in _cancel_frames(stream, machine_flow["runner_id"])
        if frame.get("run_id") == run["id"]
    ]
    assert cancels, f"no cancel frame for {run['id']}"
    assert cancels[0]["reason"] == "cancellation_pending"


def test_poll_failed_other_cause_stays_quiet(machine_client, machine_flow, daemon_world, seeder):
    run = _seed_claim(
        seeder, daemon_world, machine_flow, status="failed", error_code="heartbeat_reaped"
    )
    opened = _open_runner_session(machine_client, machine_flow["runner_id"])
    stream = _poll_once(machine_client, machine_flow, opened["session_id"], run["id"])
    cancels = [
        frame
        for frame in _cancel_frames(stream, machine_flow["runner_id"])
        if frame.get("run_id") == run["id"]
    ]
    assert cancels == []


def test_poll_live_run_stays_quiet(machine_client, machine_flow, daemon_world, seeder):
    run = _seed_claim(seeder, daemon_world, machine_flow, status="running")
    opened = _open_runner_session(machine_client, machine_flow["runner_id"])
    stream = _poll_once(machine_client, machine_flow, opened["session_id"], run["id"])
    cancels = [
        frame
        for frame in _cancel_frames(stream, machine_flow["runner_id"])
        if frame.get("run_id") == run["id"]
    ]
    assert cancels == []


def test_poll_reaper_cancelled_run_stays_quiet(
    machine_client, machine_flow, daemon_world, seeder
):
    run = _seed_claim(seeder, daemon_world, machine_flow, status="cancelled")
    opened = _open_runner_session(machine_client, machine_flow["runner_id"])
    stream = _poll_once(machine_client, machine_flow, opened["session_id"], run["id"])
    cancels = [
        frame
        for frame in _cancel_frames(stream, machine_flow["runner_id"])
        if frame.get("run_id") == run["id"]
    ]
    assert cancels == []
