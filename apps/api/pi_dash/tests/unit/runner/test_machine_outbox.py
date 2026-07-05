# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Unit tests for the per-dev-machine Redis Streams outbox.

Mirror of the per-runner outbox tests: exercise enqueue routing
(live vs offline vs reject), key namespacing, and ack — all against a
minimal fake Redis so no live server is needed.
"""

from __future__ import annotations

import pytest

from pi_dash.runner.services import machine_outbox
from pi_dash.runner.services.machine_outbox import MachineOfflineError


class _FakeRedis:
    def __init__(self):
        self.streams: dict[str, list] = {}
        self.acked: list = []
        self._seq = 0

    def xadd(self, key, fields, maxlen=None, approximate=None):
        self._seq += 1
        sid = f"{self._seq}-0".encode()
        self.streams.setdefault(key, []).append((sid, dict(fields)))
        return sid

    def xgroup_create(self, name=None, groupname=None, id=None, mkstream=None):
        return True

    def expire(self, key, ttl):
        return True

    def xack(self, key, group, *ids):
        self.acked.extend(ids)
        return len(ids)


MID = "11111111-1111-1111-1111-111111111111"


@pytest.mark.unit
def test_key_builders_are_machine_namespaced():
    assert machine_outbox.stream_key(MID) == f"machine_stream:{MID}"
    assert machine_outbox.group_name(MID) == f"machine-group:{MID}"
    assert machine_outbox.offline_stream_key(MID) == f"machine_offline_stream:{MID}"
    assert machine_outbox.consumer_name("s") == "machine-consumer-s"
    # Must not collide with the per-runner outbox namespace.
    from pi_dash.runner.services import outbox

    assert machine_outbox.stream_key(MID) != outbox.stream_key(MID)


@pytest.mark.unit
def test_enqueue_online_goes_to_live_stream(monkeypatch):
    client = _FakeRedis()
    monkeypatch.setattr(machine_outbox, "redis_instance", lambda: client)
    monkeypatch.setattr(
        machine_outbox, "active_session_id_for_machine", lambda _mid: "sess-1"
    )

    sid = machine_outbox.enqueue_for_machine(MID, {"type": "create_runner", "name": "r1"})

    assert sid == "1-0"
    assert machine_outbox.stream_key(MID) in client.streams
    assert machine_outbox.offline_stream_key(MID) not in client.streams


@pytest.mark.unit
def test_enqueue_offline_queueable_type_buffers(monkeypatch):
    client = _FakeRedis()
    monkeypatch.setattr(machine_outbox, "redis_instance", lambda: client)
    monkeypatch.setattr(
        machine_outbox, "active_session_id_for_machine", lambda _mid: None
    )

    # ``welcome`` is not in the offline-reject set → it buffers.
    result = machine_outbox.enqueue_for_machine(MID, {"type": "welcome"})

    assert result is None
    assert machine_outbox.offline_stream_key(MID) in client.streams
    assert machine_outbox.stream_key(MID) not in client.streams


@pytest.mark.unit
def test_enqueue_offline_create_runner_raises(monkeypatch):
    client = _FakeRedis()
    monkeypatch.setattr(machine_outbox, "redis_instance", lambda: client)
    monkeypatch.setattr(
        machine_outbox, "active_session_id_for_machine", lambda _mid: None
    )

    with pytest.raises(MachineOfflineError) as exc:
        machine_outbox.enqueue_for_machine(MID, {"type": "create_runner"})
    assert exc.value.dev_machine_id == MID
    assert exc.value.message_type == "create_runner"
    # Nothing written to either stream on reject.
    assert client.streams == {}


@pytest.mark.unit
def test_enqueue_unknown_type_rejected(monkeypatch):
    monkeypatch.setattr(machine_outbox, "redis_instance", lambda: _FakeRedis())
    with pytest.raises(ValueError, match="unknown machine message type"):
        machine_outbox.enqueue_for_machine(MID, {"type": "assign"})


@pytest.mark.unit
def test_ack_for_session_calls_xack(monkeypatch):
    client = _FakeRedis()
    monkeypatch.setattr(machine_outbox, "redis_instance", lambda: client)
    n = machine_outbox.ack_for_session(MID, ["1-0", "2-0", ""])
    assert n == 2
    assert client.acked == ["1-0", "2-0"]
