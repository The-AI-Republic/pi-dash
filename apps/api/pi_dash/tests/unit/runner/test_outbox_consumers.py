# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.runner.services import outbox


class _FakeRedis:
    def __init__(self, *, autoclaim_results=None, autoclaim_error=None, consumers=None):
        self.autoclaim_results = list(autoclaim_results or [])
        self.autoclaim_error = autoclaim_error
        self.consumers = consumers or []
        self.deleted = []

    def xautoclaim(self, **_kwargs):
        if self.autoclaim_error is not None:
            raise self.autoclaim_error
        if self.autoclaim_results:
            return self.autoclaim_results.pop(0)
        return (b"0-0", [])

    def xgroup_delconsumer(self, name, groupname, consumername):
        self.deleted.append((name, groupname, consumername))
        return 1

    def xinfo_consumers(self, _name, _groupname):
        return self.consumers


@pytest.mark.unit
def test_claim_pending_deletes_old_consumer_after_full_handoff(monkeypatch):
    client = _FakeRedis(
        autoclaim_results=[
            (b"0-0", [b"1-0", b"2-0"]),
        ]
    )
    monkeypatch.setattr(outbox, "redis_instance", lambda: client)

    claimed = outbox.claim_pending_for_new_session(
        "runner-1",
        old_consumer="consumer-old",
        new_consumer="consumer-new",
    )

    assert claimed == 2
    assert client.deleted == [
        (
            "runner_stream:runner-1",
            "runner-group:runner-1",
            "consumer-old",
        )
    ]


@pytest.mark.unit
def test_claim_pending_keeps_old_consumer_when_handoff_fails(monkeypatch):
    client = _FakeRedis(autoclaim_error=RuntimeError("redis unavailable"))
    monkeypatch.setattr(outbox, "redis_instance", lambda: client)

    claimed = outbox.claim_pending_for_new_session(
        "runner-1",
        old_consumer="consumer-old",
        new_consumer="consumer-new",
    )

    assert claimed == 0
    assert client.deleted == []


@pytest.mark.unit
def test_reap_idle_consumers_drops_only_stale_zero_pending_consumers(monkeypatch):
    client = _FakeRedis(
        consumers=[
            {"name": b"consumer-active", "pending": 0, "idle": 900_000},
            {"name": b"consumer-pending", "pending": 1, "idle": 900_000},
            {"name": b"consumer-fresh", "pending": 0, "idle": 1_000},
            {"name": b"consumer-stale", "pending": 0, "idle": 900_000},
        ]
    )
    monkeypatch.setattr(outbox, "redis_instance", lambda: client)

    removed = outbox.reap_idle_consumers(
        "runner-1",
        keep_consumers={"consumer-active"},
        min_idle_ms=120_000,
    )

    assert removed == 1
    assert client.deleted == [
        (
            "runner_stream:runner-1",
            "runner-group:runner-1",
            "consumer-stale",
        )
    ]
