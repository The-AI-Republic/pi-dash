# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

import pytest

from pi_dash.runner import tasks
from pi_dash.runner.services import outbox


class _FakeValues:
    def __init__(self, rows):
        self.rows = rows

    def __iter__(self):
        return iter(self.rows)

    def distinct(self):
        return self


class _FakeQuery:
    def __init__(self, rows):
        self.rows = rows

    def values_list(self, *_fields, **_kwargs):
        return _FakeValues(self.rows)


class _FakeSessionManager:
    def __init__(self, active_rows, runner_ids):
        self.active_rows = active_rows
        self.runner_ids = runner_ids

    def filter(self, **kwargs):
        assert kwargs == {"revoked_at__isnull": True}
        return _FakeQuery(self.active_rows)

    def values_list(self, *_fields, **_kwargs):
        return _FakeValues(self.runner_ids)


@pytest.mark.unit
def test_sweep_old_streams_reaps_consumers_for_revoked_sessions(monkeypatch):
    active_runner_id = "active-runner"
    revoked_runner_id = "revoked-runner"
    active_session_id = "active-session"
    trim_calls = []
    reap_calls = []

    class _FakeRunnerSession:
        objects = _FakeSessionManager(
            active_rows=[(active_runner_id, active_session_id)],
            runner_ids=[active_runner_id, revoked_runner_id],
        )

    monkeypatch.setattr(tasks, "RunnerSession", _FakeRunnerSession)
    monkeypatch.setattr(outbox, "id_for_secs_ago", lambda _secs: "cutoff-id")
    monkeypatch.setattr(
        outbox,
        "safe_trim_runner_stream",
        lambda runner_id, time_cutoff_id: trim_calls.append(
            (runner_id, time_cutoff_id)
        )
        or 0,
    )
    monkeypatch.setattr(
        outbox,
        "reap_idle_consumers",
        lambda runner_id, keep_consumers: reap_calls.append(
            (runner_id, set(keep_consumers))
        )
        or 0,
    )
    monkeypatch.setattr(outbox, "due_runners_for_stream_cleanup", lambda: [])

    assert tasks.sweep_old_streams.run() == 0

    assert trim_calls == [(active_runner_id, "cutoff-id")]
    assert (active_runner_id, {outbox.consumer_name(active_session_id)}) in reap_calls
    assert (revoked_runner_id, set()) in reap_calls
