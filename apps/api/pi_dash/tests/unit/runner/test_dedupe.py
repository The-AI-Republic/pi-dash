# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.runner.consumers import RunnerConsumer


class FakeRunner:
    id = "runner-x"


@pytest.mark.unit
def test_message_id_dedupe_detects_repeat():
    c = RunnerConsumer()
    msg = {"mid": "m1", "type": "heartbeat"}
    assert c._is_duplicate(msg) is False
    assert c._is_duplicate(msg) is True


@pytest.mark.unit
def test_message_without_mid_never_duplicates():
    c = RunnerConsumer()
    msg = {"type": "heartbeat"}
    assert c._is_duplicate(msg) is False
    assert c._is_duplicate(msg) is False


@pytest.mark.unit
def test_seq_monotonic_enforced_for_run_events():
    c = RunnerConsumer()
    runner = FakeRunner()
    base = {"type": "run_event", "run_id": "r1"}
    assert c._seq_ok(runner, {**base, "seq": 1}) is True
    assert c._seq_ok(runner, {**base, "seq": 2}) is True
    # Replay or out-of-order: drop.
    assert c._seq_ok(runner, {**base, "seq": 2}) is False
    assert c._seq_ok(runner, {**base, "seq": 1}) is False
    # Forward jump with a gap is accepted.
    assert c._seq_ok(runner, {**base, "seq": 10}) is True


@pytest.mark.unit
def test_seq_check_ignored_for_non_run_events():
    c = RunnerConsumer()
    runner = FakeRunner()
    assert c._seq_ok(runner, {"type": "heartbeat"}) is True
