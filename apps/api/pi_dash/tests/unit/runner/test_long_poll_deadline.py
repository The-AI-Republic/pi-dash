# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Regression: the long-poll loop must honor its deadline.

Background: ``XREADGROUP BLOCK 0 STREAMS … >`` means "block forever" in
Redis (BLOCK 0 is documented as "block indefinitely"), not "do not
block." If the poll loop's deadline expires and we still call
``outbox.aread_for_session(block_ms=0)``, Redis parks the request
indefinitely — the request is stuck, the runner's HTTP timeout
fires unnoticed, and any new assign message that lands later is claimed
into the dead consumer's PEL where the live session can never see it.

Observed in production: a poll handler stuck for 4+ hours on an evicted
session caused an unrelated run to be reaped because the live runner's
own poll couldn't grab a worker, and the assign message ended up in the
dead consumer's PEL.

The fix: bail with `[]` BEFORE calling Redis whenever the deadline has
expired.

The wait loop is async (``_aread_with_eviction_awareness``) so the block
window parks on the event loop instead of pinning the per-process
``thread_sensitive`` thread; these tests drive it with ``asyncio.run``.
"""

from __future__ import annotations

import asyncio
from typing import Any
from unittest.mock import patch

import pytest
from redis.exceptions import ConnectionError as RedisConnectionError

from pi_dash.runner.views.sessions import (
    _aread_with_eviction_awareness,
    _session_open_side_effect,
)


@pytest.mark.unit
def test_session_open_side_effect_returns_none_on_redis_error():
    def _raise():
        raise TimeoutError("redis timed out")

    assert _session_open_side_effect("runner-1", "claim", _raise) is None


@pytest.mark.unit
def test_session_open_side_effect_reraises_programming_errors():
    def _raise():
        raise TypeError("wrong helper signature")

    with pytest.raises(TypeError, match="wrong helper signature"):
        _session_open_side_effect("runner-1", "claim", _raise)


@pytest.mark.unit
def test_loop_returns_empty_without_calling_redis_when_deadline_expired():
    """When `block_ms=0` arrives at the loop entry, never invoke Redis.

    Asserts the structural fix: the deadline check happens BEFORE
    `aread_for_session` — not after — so the helper cannot accidentally
    issue ``XREADGROUP BLOCK 0`` (= block forever) on the
    deadline-already-expired path.
    """
    runner_id = "11111111-1111-1111-1111-111111111111"
    session_id = "22222222-2222-2222-2222-222222222222"

    read_calls: list[dict[str, Any]] = []

    async def _spy_aread_for_session(**kwargs):
        read_calls.append(kwargs)
        return []

    # Mock async_redis_instance so the helper takes the eviction-aware
    # branch rather than the no-redis fallback. The mock object only
    # needs to support `pubsub()` returning an object with awaitable
    # `subscribe()`, `get_message()`, and `close()`.
    class _FakePubsub:
        async def subscribe(self, _channel):
            pass

        async def get_message(self, timeout=0):
            return None

        async def close(self):
            pass

    class _FakeRedis:
        def pubsub(self, ignore_subscribe_messages=True):
            return _FakePubsub()

    with (
        patch(
            "pi_dash.runner.services.outbox.aread_for_session",
            side_effect=_spy_aread_for_session,
        ),
        patch(
            "pi_dash.settings.redis.async_redis_instance",
            return_value=_FakeRedis(),
        ),
    ):
        result = asyncio.run(
            _aread_with_eviction_awareness(
                runner_id=runner_id,
                session_id=session_id,
                block_ms=0,  # ← deadline already expired at entry
                use_zero=False,
            )
        )

    assert result == []
    # The structural invariant: when the budget is zero, the loop must
    # not have called Redis. If this fires, the deadline check has
    # regressed to AFTER the read and we've reintroduced the BLOCK 0
    # forever-park bug.
    assert read_calls == [], (
        f"aread_for_session must not be called when block_ms=0; got: {read_calls}"
    )


@pytest.mark.unit
def test_initial_pel_replay_still_reads_with_zero_budget():
    """`use_zero=True` is a nonblocking PEL replay, not a long poll for `>`."""
    runner_id = "11111111-1111-1111-1111-111111111111"
    session_id = "22222222-2222-2222-2222-222222222222"
    replayed = [{"stream_id": "1-0", "type": "assign"}]
    read_calls: list[dict[str, Any]] = []

    async def _spy_aread_for_session(**kwargs):
        read_calls.append(kwargs)
        return replayed

    with patch(
        "pi_dash.runner.services.outbox.aread_for_session",
        side_effect=_spy_aread_for_session,
    ):
        result = asyncio.run(
            _aread_with_eviction_awareness(
                runner_id=runner_id,
                session_id=session_id,
                block_ms=0,
                use_zero=True,
            )
        )

    assert result == replayed
    assert read_calls == [
        {
            "runner_id": runner_id,
            "session_id": session_id,
            "block_ms": 0,
            "count": 100,
            "use_zero": True,
        }
    ]


@pytest.mark.unit
def test_pubsub_is_closed_when_subscribe_fails():
    runner_id = "11111111-1111-1111-1111-111111111111"
    session_id = "22222222-2222-2222-2222-222222222222"

    class _FailingPubsub:
        closed = False

        async def subscribe(self, _channel):
            raise RedisConnectionError("too many connections")

        async def close(self):
            self.closed = True

    pubsub = _FailingPubsub()

    class _FakeRedis:
        def pubsub(self, ignore_subscribe_messages=True):
            return pubsub

    with patch("pi_dash.settings.redis.async_redis_instance", return_value=_FakeRedis()):
        with pytest.raises(RedisConnectionError, match="too many connections"):
            asyncio.run(
                _aread_with_eviction_awareness(
                    runner_id=runner_id,
                    session_id=session_id,
                    block_ms=100,
                    use_zero=False,
                )
            )

    assert pubsub.closed is True
