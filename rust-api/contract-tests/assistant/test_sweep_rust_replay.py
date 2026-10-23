# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Stale-turn sweep oracle vs the Rust worker (PIDASHCONV-1137).

The Django oracle for this domain is ``assistant/test_tasks_turn.py``
(``test_sweep_fails_stale_turn`` / ``test_sweep_ignores_fresh_and_terminal``,
run by the worker-plane oracle job in ``rust-api-contract-tests.yml``). This
file proves the Rust side of the same DB contract: ``assistant.sweep_stale_turns``
executes LOCALLY in the Rust worker (``worker()`` wired the handler in
PIDASHCONV-831), so the replay seeds ``rust_job_queue`` rows with the exact
Celery v2 payload (``_harness/rust_queue.py``) and diffs the database before
and after. The Rust worker (``pidash-api worker``) must be running against the
same ``DATABASE_URL``; no Celery worker, broker or HTTP server is involved.

Regression pinned here (PIDASHCONV-1137): the sweep's fail path writes an
``assistant_message`` error row plus the matching ``assistant_event``
``turn_failed`` row. ``created_at`` on both is Django ``auto_now_add``
(``NOT NULL``, no database default in the migrated schema), and the two raw
inserts omitted it, so on a live database every stale turn parked on
``null value in column "created_at" ... violates not-null constraint`` instead
of failing. The assertions below require ``created_at`` on both rows — without
the fix the sweep never settles and the turn stays ``running``, so the test
fails.

Regression pinned here (PIDASHCONV-1248): the sweep cut at a hard-coded
330+60s, ignoring ``ASSISTANT_TURN_HARD_LIMIT``. The worker boots with the
limit at 900 (see the replay job env) and
``test_sweep_honors_configured_hard_limit`` seeds turns on both sides of the
960s cutoff — pre-fix the young turn is swept and the test fails.
"""

from __future__ import annotations

import os
import time
import uuid

from psycopg.rows import dict_row

from _harness import rust_queue
from _harness import seed as seed_helpers
from _harness.db import wait_for_condition as wait_for

SWEEP = "assistant.sweep_stale_turns"

#: ``sweep_stale_turns`` fails ``RUNNING`` rows older than
#: ``TURN_HARD_LIMIT + 60`` (330 + 60); the Python oracle seeds year 2000 so
#: the row is stale under any sane limit override.
ANCIENT = "2000-01-01T00:00:00Z"

TIMEOUT_CODE = "turn_timeout"
TIMEOUT_DETAIL = "The assistant turn did not finish (worker lost)."


# --------------------------------------------------------------------------- #
# Live oracle helpers (the Django oracle's shapes, minus the HTTP client)
# --------------------------------------------------------------------------- #


def _now_iso() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def _seed_member(db_conn, tag: str):
    """One user + workspace + membership; returns (user, workspace)."""
    seed_helpers.ensure_setup_done(db_conn)
    user = seed_helpers.user(db_conn, f"sweep-{tag}")
    workspace = seed_helpers.workspace(db_conn, f"sweep-{tag}", user["id"])
    seed_helpers.workspace_member(db_conn, workspace["id"], user["id"], role=15)
    return user, workspace


def _seed_turn_trio(db_conn, user, workspace, *, status="queued", started_at=None):
    """Replicate the message-POST 202 rows (``views/messages.py``).

    Thread (with ``active_turn`` set) + user message (seq 1, completed) +
    turn pointing at the message. Returns (thread_id, turn_id, message_id).
    """
    thread_id, turn_id, message_id = (str(uuid.uuid4()) for _ in range(3))
    now = _now_iso()
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO assistant_thread "
            "(id, workspace_id, user_id, title, kind, is_archived, "
            "created_at, updated_at) "
            "VALUES (%s, %s, %s, %s, 'chat', FALSE, %s, %s)",
            (thread_id, str(workspace["id"]), str(user["id"]), "sweep oracle", now, now),
        )
        cur.execute(
            # model_used / error_code / error_detail are Django-side
            # defaults (""), not DB defaults: raw SQL must pass them.
            "INSERT INTO assistant_turn (id, thread_id, status, model_used, error_code, "
            "error_detail, created_at, started_at) "
            "VALUES (%s, %s, %s, '', '', '', %s, %s)",
            (turn_id, thread_id, status, now, started_at),
        )
        cur.execute(
            "INSERT INTO assistant_message "
            "(id, thread_id, turn_id, seq, kind, display_content, payload, status, created_at) "
            "VALUES (%s, %s, %s, 1, 'user', 'do a thing', '{}', 'completed', %s)",
            (message_id, thread_id, turn_id, now),
        )
        cur.execute(
            "UPDATE assistant_turn SET user_message_id = %s WHERE id = %s",
            (message_id, turn_id),
        )
        cur.execute(
            "UPDATE assistant_thread SET active_turn_id = %s WHERE id = %s",
            (turn_id, thread_id),
        )
    return thread_id, turn_id, message_id


def _seed_streaming_row(db_conn, thread_id, turn_id):
    """A still-``streaming`` assistant row: ``_finalize_open_rows`` closes it."""
    message_id = str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO assistant_message "
            "(id, thread_id, turn_id, seq, kind, display_content, payload, status, created_at) "
            "VALUES (%s, %s, %s, 2, 'assistant', 'partial', '{}', 'streaming', %s)",
            (message_id, thread_id, turn_id, _now_iso()),
        )
    return message_id


def _seed_delta_event(db_conn, thread_id, turn_id):
    """A prunable ``assistant_delta`` event."""
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO assistant_event (thread_id, turn_id, seq, kind, payload, created_at) "
            "VALUES (%s, %s, 1, 'assistant_delta', '{}', %s)",
            (thread_id, turn_id, _now_iso()),
        )


def _turn_row(db_conn, turn_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT * FROM assistant_turn WHERE id = %s", (turn_id,))
        return cur.fetchone()


def _thread_row(db_conn, thread_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT * FROM assistant_thread WHERE id = %s", (thread_id,))
        return cur.fetchone()


def _turn_messages(db_conn, turn_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT id, kind, status, display_content, created_at, completed_at "
            "FROM assistant_message WHERE turn_id = %s ORDER BY seq",
            (turn_id,),
        )
        return cur.fetchall()


def _turn_events(db_conn, turn_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT kind, payload, message_id, created_at FROM assistant_event "
            "WHERE turn_id = %s ORDER BY seq",
            (turn_id,),
        )
        return cur.fetchall()


def _publish_sweep() -> None:
    """Seed one sweep job and wait for the Rust worker to settle it."""
    celery_id = rust_queue.publish(SWEEP, [], {})
    rust_queue.wait_for_settled(celery_id, what="assistant sweep settled")


# --------------------------------------------------------------------------- #
# Live oracle: publish into rust_job_queue, diff the DB
# --------------------------------------------------------------------------- #


def test_sweep_fails_stale_turn_and_stamps_created_at(db_conn):
    """A RUNNING turn older than 330+60s fails with ``turn_timeout``.

    The fail path's two inserts must carry ``created_at`` (the regression):
    the error message row and the ``turn_failed`` event row are asserted on it.
    """
    user, workspace = _seed_member(db_conn, "stale")
    thread_id, turn_id, _msg = _seed_turn_trio(
        db_conn, user, workspace, status="running", started_at=ANCIENT
    )
    streaming_id = _seed_streaming_row(db_conn, thread_id, turn_id)
    _seed_delta_event(db_conn, thread_id, turn_id)

    _publish_sweep()

    row = wait_for(
        lambda: _turn_row(db_conn, turn_id)
        if _turn_row(db_conn, turn_id)["status"] == "failed"
        else None,
        what="stale turn swept",
    )
    assert row["error_code"] == TIMEOUT_CODE
    assert row["error_detail"] == TIMEOUT_DETAIL
    assert row["completed_at"] is not None
    assert _thread_row(db_conn, thread_id)["active_turn_id"] is None

    # `_finalize_open_rows`: the in-flight row closes with the terminal status.
    by_id = {str(m["id"]): m for m in _turn_messages(db_conn, turn_id)}
    assert by_id[streaming_id]["status"] == "failed"
    assert by_id[streaming_id]["completed_at"] is not None

    # The error message row (`events.create_message`): `error`/`failed`,
    # carrying the detail, `completed_at` NULL, and — the regression —
    # `created_at` stamped.
    errors = [m for m in by_id.values() if m["kind"] == "error"]
    assert len(errors) == 1, "exactly one error row per failed turn"
    error = errors[0]
    assert error["status"] == "failed"
    assert error["display_content"] == TIMEOUT_DETAIL
    assert error["completed_at"] is None
    assert error["created_at"] is not None, (
        "assistant_message.created_at is auto_now_add: the insert must stamp it"
    )

    # The `turn_failed` event row (`events.append_event`) points at the error
    # message and — the regression — is stamped too. Deltas are pruned.
    events = _turn_events(db_conn, turn_id)
    kinds = [e["kind"] for e in events]
    assert "turn_failed" in kinds
    assert "assistant_delta" not in kinds, "deltas are pruned on failure"
    failed = next(e for e in events if e["kind"] == "turn_failed")
    assert str(failed["message_id"]) == str(error["id"])
    assert failed["payload"] == {
        "turn_id": turn_id,
        "error_code": TIMEOUT_CODE,
        "detail": TIMEOUT_DETAIL,
    }
    assert failed["created_at"] is not None, (
        "assistant_event.created_at is auto_now_add: the insert must stamp it"
    )


def test_sweep_ignores_fresh_and_terminal(db_conn):
    """Fresh RUNNING rows and terminal rows are untouched by the sweep."""
    user, workspace = _seed_member(db_conn, "fresh")
    fresh_thread, fresh_turn, _m1 = _seed_turn_trio(
        db_conn, user, workspace, status="running", started_at=_now_iso()
    )
    done_thread, done_turn, _m2 = _seed_turn_trio(
        db_conn, user, workspace, status="completed", started_at=ANCIENT
    )

    _publish_sweep()

    assert _turn_row(db_conn, fresh_turn)["status"] == "running"
    assert _turn_row(db_conn, done_turn)["status"] == "completed"
    assert _turn_row(db_conn, done_turn)["completed_at"] is None, (
        "the sweep never re-writes a terminal row"
    )
    assert str(_thread_row(db_conn, fresh_thread)["active_turn_id"]) == str(fresh_turn)
    assert [e for e in _turn_events(db_conn, fresh_turn) if e["kind"] == "turn_failed"] == []
    assert [e for e in _turn_events(db_conn, done_turn) if e["kind"] == "turn_failed"] == []


def _iso_age(age_secs: int) -> str:
    """UTC ISO instant ``age_secs`` in the past (a turn's ``started_at``)."""
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(time.time() - age_secs))


def test_sweep_honors_configured_hard_limit(db_conn):
    """The sweep cuts at ASSISTANT_TURN_HARD_LIMIT + 60 (PIDASHCONV-1248).

    The Rust worker under test boots with ASSISTANT_TURN_HARD_LIMIT=900 (see
    the sweep-rust replay job env): a turn younger than the 960s cutoff
    survives the sweep while a turn older than it fails with ``turn_timeout``.
    The ages derive from the configured value, so the same test pins the
    default 390s horizon when the override is absent. Pre-fix the worker
    hard-codes 330 and sweeps the young turn, so this test fails.
    """
    limit = int(os.environ.get("ASSISTANT_TURN_HARD_LIMIT", "330"))
    cutoff = limit + 60
    user, workspace = _seed_member(db_conn, "hardlimit")
    _young_thread, young_turn, _m1 = _seed_turn_trio(
        db_conn, user, workspace, status="running", started_at=_iso_age(cutoff - 30)
    )
    _old_thread, old_turn, _m2 = _seed_turn_trio(
        db_conn, user, workspace, status="running", started_at=_iso_age(cutoff + 30)
    )

    _publish_sweep()

    row = wait_for(
        lambda: _turn_row(db_conn, old_turn)
        if _turn_row(db_conn, old_turn)["status"] == "failed"
        else None,
        what="over-cutoff turn swept",
    )
    assert row["error_code"] == TIMEOUT_CODE
    assert row["error_detail"] == TIMEOUT_DETAIL
    # Settle: the sweep may still be running when the old row flips.
    time.sleep(2)
    assert _turn_row(db_conn, young_turn)["status"] == "running", (
        f"a turn younger than the configured {cutoff}s cutoff must survive the sweep"
    )
    assert [e for e in _turn_events(db_conn, young_turn) if e["kind"] == "turn_failed"] == []
