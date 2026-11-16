# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Loop fire oracle vs the Rust worker (PIDASHCONV-1220).

The Django oracle for this behaviour is
``loop/test_worker.py::test_fire_happy_path_creates_hidden_turn`` (run by the
Django worker path, listed in ``TASK_ORACLE_IGNORES.txt`` as local-only). This
file proves the Rust side of the same DB contract: ``pi_dash.bgtasks.loop
.fire_loop_target`` executes LOCALLY in the Rust worker (``loop_fire`` is
registered in ``register_oss_handlers``), so the replay seeds
``rust_job_queue`` rows with the exact Celery v2 payload
(``_harness/rust_queue.py``) and diffs the database before and after. The
Rust worker (``pidash-api worker``) must be running against the same
``DATABASE_URL``; no Celery worker, broker or HTTP server is involved.

Regression pinned here (PIDASHCONV-1220): the dispatch turn insert
(``crates/jobs/src/loop/dispatch.rs``) wrote only
``(id, thread_id, status, created_at)``. Django's ORM fills
``model_used`` / ``error_code`` / ``error_detail`` from the model defaults
(``""``), but the columns are ``NOT NULL`` with no database default, so on a
live database every eligible fire parked on
``null value in column "model_used" ... violates not-null constraint``: the
dispatch transaction rolled back and ``record_dispatch_error`` stamped
``last_skip_reason='dispatch_error'``. The assertions below require the three
columns on the queued turn and ``last_skip_reason=''`` on the target, so
without the fix the fire never settles into a hidden turn.

The Rust worker owns ``assistant.run_turn`` (unlike the Django oracle, which
leaves it on an unconsumed queue), so the turn can leave ``queued`` as soon as
the worker claims it. An AFTER INSERT capture trigger records the row exactly
as dispatch committed it; that is the state the issue's Done-when names.
"""

from __future__ import annotations

import uuid

from psycopg.rows import dict_row

from _harness import rust_queue
from _harness import seed as seed_helpers
from _harness.db import wait_for_condition as wait_for

from . import seed_loop as seed_l

FIRE = "pi_dash.bgtasks.loop.fire_loop_target"

CAPTURE_TABLE = "contract_turn_insert_capture_1220"


def _install_capture(db_conn) -> None:
    """Record every ``assistant_turn`` INSERT as dispatch committed it.

    The worker's ``assistant.run_turn`` handler may claim the turn within
    milliseconds of the dispatch commit, so reading the live row cannot
    reliably observe the insert values the contract is about.
    """
    db_conn.execute(
        f"CREATE TABLE IF NOT EXISTS {CAPTURE_TABLE} (\n"
        "  turn_id uuid PRIMARY KEY,\n"
        "  thread_id uuid NOT NULL,\n"
        "  status text NOT NULL,\n"
        "  model_used text,\n"
        "  error_code text,\n"
        "  error_detail text,\n"
        "  user_message_id uuid,\n"
        "  captured_at timestamptz NOT NULL DEFAULT now()\n"
        ")"
    )
    db_conn.execute(
        "CREATE OR REPLACE FUNCTION contract_capture_turn_insert_1220()\n"
        "RETURNS trigger LANGUAGE plpgsql AS $$\n"
        "BEGIN\n"
        f"  INSERT INTO {CAPTURE_TABLE}\n"
        "    (turn_id, thread_id, status, model_used, error_code,\n"
        "     error_detail, user_message_id)\n"
        "  VALUES (NEW.id, NEW.thread_id, NEW.status, NEW.model_used,\n"
        "          NEW.error_code, NEW.error_detail, NEW.user_message_id);\n"
        "  RETURN NEW;\n"
        "END $$"
    )
    db_conn.execute(
        "DROP TRIGGER IF EXISTS contract_capture_turn_insert_1220 "
        "ON assistant_turn"
    )
    db_conn.execute(
        "CREATE TRIGGER contract_capture_turn_insert_1220 "
        "AFTER INSERT ON assistant_turn FOR EACH ROW "
        "EXECUTE FUNCTION contract_capture_turn_insert_1220()"
    )


def _drop_capture(db_conn) -> None:
    db_conn.execute(
        "DROP TRIGGER IF EXISTS contract_capture_turn_insert_1220 "
        "ON assistant_turn"
    )
    db_conn.execute("DROP FUNCTION IF EXISTS contract_capture_turn_insert_1220()")


def _seed_member(db_conn, tag: str):
    """One eligible workspace member: LLM config + MEMBER role."""
    seed_helpers.ensure_setup_done(db_conn)
    user = seed_helpers.user(db_conn, f"loopfire-{tag}")
    workspace = seed_helpers.workspace(db_conn, f"loopfire-{tag}", user["id"])
    seed_helpers.workspace_member(
        db_conn, workspace["id"], user["id"], role=seed_l.MEMBER
    )
    seed_l.configure_llm(db_conn, user_id=user["id"])
    return user, workspace


def _target_row(db_conn, target_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT last_run_id, thread_id, last_skip_reason, next_run_at "
            "FROM loop_targets WHERE id = %s",
            (str(target_id),),
        )
        return cur.fetchone()


def _thread_row(db_conn, thread_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT kind, title, is_archived "
            "FROM assistant_thread WHERE id = %s",
            (str(thread_id),),
        )
        return cur.fetchone()


def _captured_turn(db_conn, turn_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT status, thread_id, user_message_id, model_used, "
            "error_code, error_detail FROM "
            f"{CAPTURE_TABLE} WHERE turn_id = %s",
            (str(turn_id),),
        )
        return cur.fetchone()


def _live_turn_message_id(db_conn, turn_id):
    with db_conn.cursor() as cur:
        cur.execute(
            "SELECT user_message_id FROM assistant_turn WHERE id = %s",
            (str(turn_id),),
        )
        row = cur.fetchone()
        return row[0] if row else None


def _message_row(db_conn, message_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT kind, display_content, status "
            "FROM assistant_message WHERE id = %s",
            (str(message_id),),
        )
        return cur.fetchone()


def _publish_fire(target_id) -> None:
    rust_queue.publish(FIRE, [str(target_id)], {})


def test_fire_happy_path_creates_hidden_turn(db_conn):
    """A due, eligible fire creates the queued turn Django would create."""
    tag = uuid.uuid4().hex[:10]
    _install_capture(db_conn)
    try:
        user, workspace = _seed_member(db_conn, tag)
        job = seed_l.create_loop_job(
            db_conn,
            slug=f"loopfire-{tag}",
            public_name="Fire Happy",
            prompt="DO THE THING",
        )
        target = seed_l.create_target(
            db_conn,
            job_id=job["id"],
            workspace_id=workspace["id"],
            user_id=user["id"],
            next_run_at=seed_l.hours_ago(1),
        )
        before_cursor = _target_row(db_conn, target["id"])["next_run_at"]

        _publish_fire(target["id"])

        def fired():
            row = _target_row(db_conn, target["id"])
            return row if row["last_run_id"] is not None else None

        row = wait_for(fired, what="loop fire dispatched a hidden turn")
        assert row["last_skip_reason"] == "", (
            "an eligible fire must dispatch, not record a skip reason"
        )
        assert str(row["next_run_at"]) > str(before_cursor)

        thread = _thread_row(db_conn, row["thread_id"])
        assert thread["kind"] == "loop"
        assert thread["title"] == "Fire Happy"
        assert thread["is_archived"] is False

        turn = _captured_turn(db_conn, row["last_run_id"])
        assert turn is not None, (
            "dispatch committed no assistant_turn row"
        )
        assert str(turn["thread_id"]) == str(row["thread_id"])
        assert turn["status"] == "queued"
        # model_used / error_code / error_detail are Django-side defaults
        # ("") on NOT NULL columns with no database default: the raw insert
        # must carry them explicitly.
        assert turn["model_used"] == ""
        assert turn["error_code"] == ""
        assert turn["error_detail"] == ""

        message_id = _live_turn_message_id(db_conn, row["last_run_id"])
        assert message_id is not None, "dispatch never linked the prompt message"
        message = _message_row(db_conn, message_id)
        assert (
            message["kind"], message["display_content"], message["status"]
        ) == ("user", "DO THE THING", "completed")
    finally:
        _drop_capture(db_conn)
