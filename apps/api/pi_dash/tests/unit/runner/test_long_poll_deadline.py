# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Regression: the long-poll loop must honor its deadline.

Background: ``XREADGROUP BLOCK 0 STREAMS … >`` means "block forever" in
Redis (BLOCK 0 is documented as "block indefinitely"), not "do not
block." If the poll loop's deadline expires and we still call
``outbox.read_for_session(block_ms=0)``, Redis parks the worker
indefinitely — the gunicorn worker is stuck, the runner's HTTP timeout
fires unnoticed, and any new assign message that lands later is claimed
into the dead consumer's PEL where the live session can never see it.

Observed in production: a poll handler stuck for 4+ hours on an evicted
session caused an unrelated run to be reaped because the live runner's
own poll couldn't grab a worker, and the assign message ended up in the
dead consumer's PEL.

The fix: bail with `[]` BEFORE calling Redis whenever the deadline has
expired.
"""

from __future__ import annotations

from typing import Any
from unittest.mock import patch

import pytest

from pi_dash.runner.views.sessions import RunnerSessionPollEndpoint


@pytest.mark.unit
def test_loop_returns_empty_without_calling_redis_when_deadline_expired():
    """When `block_ms=0` arrives at the loop entry, never invoke Redis.

    Asserts the structural fix: the deadline check happens BEFORE
    `read_for_session` — not after — so the helper cannot accidentally
    issue ``XREADGROUP BLOCK 0`` (= block forever) on the
    deadline-already-expired path.
    """
    endpoint = RunnerSessionPollEndpoint()
    runner_id = "11111111-1111-1111-1111-111111111111"
    session_id = "22222222-2222-2222-2222-222222222222"

    read_calls: list[dict[str, Any]] = []

    def _spy_read_for_session(**kwargs):
        read_calls.append(kwargs)
        return []

    # Mock redis_instance so the helper takes the eviction-aware branch
    # rather than the no-redis fallback. The mock object only needs to
    # support `pubsub()` returning an object with `subscribe()`,
    # `get_message()`, and `close()`.
    class _FakePubsub:
        def subscribe(self, _channel):
            pass

        def get_message(self, timeout=0):
            return None

        def close(self):
            pass

    class _FakeRedis:
        def pubsub(self, ignore_subscribe_messages=True):
            return _FakePubsub()

    with (
        patch(
            "pi_dash.runner.views.sessions.outbox.read_for_session",
            side_effect=_spy_read_for_session,
        ),
        patch(
            "pi_dash.settings.redis.redis_instance",
            return_value=_FakeRedis(),
        ),
    ):
        result = endpoint._read_with_eviction_awareness(
            runner_id=runner_id,
            session_id=session_id,
            sid=session_id,
            block_ms=0,  # ← deadline already expired at entry
            use_zero=False,
        )

    assert result == []
    # The structural invariant: when the budget is zero, the loop must
    # not have called Redis. If this fires, the deadline check has
    # regressed to AFTER the read and we've reintroduced the BLOCK 0
    # forever-park bug.
    assert read_calls == [], (
        f"read_for_session must not be called when block_ms=0; got: {read_calls}"
    )


@pytest.mark.unit
def test_initial_pel_replay_still_reads_with_zero_budget():
    """`use_zero=True` is a nonblocking PEL replay, not a long poll for `>`."""
    endpoint = RunnerSessionPollEndpoint()
    runner_id = "11111111-1111-1111-1111-111111111111"
    session_id = "22222222-2222-2222-2222-222222222222"
    replayed = [{"stream_id": "1-0", "type": "assign"}]
    read_calls: list[dict[str, Any]] = []

    def _spy_read_for_session(**kwargs):
        read_calls.append(kwargs)
        return replayed

    with patch(
        "pi_dash.runner.views.sessions.outbox.read_for_session",
        side_effect=_spy_read_for_session,
    ):
        result = endpoint._read_with_eviction_awareness(
            runner_id=runner_id,
            session_id=session_id,
            sid=session_id,
            block_ms=0,
            use_zero=True,
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
