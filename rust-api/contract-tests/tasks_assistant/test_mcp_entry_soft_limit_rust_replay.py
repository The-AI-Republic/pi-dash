# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""MCP-entry soft-limit timeout vs the Rust worker (PIDASHCONV-1572).

The Django oracle is
``assistant/test_tasks_turn.py::test_mcp_entry_hang_fails_at_soft_limit``:
with ten MCP servers that accept the TCP connection but never answer,
toolset entry spans ~100s (sequential, ~10s per server) and the 60s soft
limit fires mid-entry, so the turn fails ``turn_timeout`` before any
provider request. This file proves the Rust side of the same contract:
``assistant.run_turn`` executes LOCALLY in the Rust worker (``worker()``
wired the handler in PIDASHCONV-863), so the replay seeds a
``rust_job_queue`` row with the exact Celery v2 payload
(``_harness/rust_queue.py``) and diffs the database. The Rust worker
(``pidash-api worker``) must be running against the same ``DATABASE_URL``
with ``ASSISTANT_TURN_SOFT_LIMIT=60`` (see the MCP-entry replay job env;
the job keeps the default hard limit — its 390s sweep cutoff stays clear
of the ~60s turn); no Celery worker, broker or HTTP server is involved.

Regression pinned here: when the soft limit fired while ``open_stream``
was still entering MCP sessions, the live seam's per-turn record (the
decrypted BYOK key and the MCP sessions) was never reclaimed and stayed in
the process-wide map for the life of the worker. The turn already failed
``turn_timeout`` correctly, so this replay pins the failure path the fix
touches; the reclaim itself is proved by the Rust unit test
(``turn::live::tests::open_stream_timeout_during_mcp_entry_reclaims_the_record``),
which fails before the fix and passes after.

A dedicated job boots the worker (the shared replay job must keep the 300
default for ``test_slow_stream_fails_at_soft_limit``), so this file carries
its own copy of the replay helpers (the
``test_sweep_rust_replay.py`` shape), not an import of the
default-env suite.
"""

from __future__ import annotations

import os
import socket
import threading
import time
import uuid

from psycopg.rows import dict_row

from _harness import rust_queue
from _harness import seed as seed_helpers
from _harness.db import wait_for_condition as wait_for

RUN_TURN = "assistant.run_turn"

API_KEY = "test-key-123"
PROVIDER_KIND = "openai_compatible"
MODEL_NAME = "oracle-stub"

#: MCP-entry timeout (PIDASHCONV-1572): the turn's ten MCP servers accept
#: the TCP connection but never answer, and toolsets are entered
#: sequentially at ~10s (the pinned init timeout) each, so entry spans
#: ~100s. The worker boots with a 60s soft limit, which deterministically
#: lands mid-entry (never past it, never the sweep).
MCP_ENTRY_SOFT_LIMIT_SECS = 60
MCP_ENTRY_HANGING_SERVERS = 10

TIMEOUT_CODE = "turn_timeout"
TIMEOUT_DETAIL = "The assistant took too long to respond."

#: Never dialed: the turn hangs in MCP entry before any provider request.
#: If the turn ever reached the provider, the refused connection would fail
#: it with a different code and this test would fail.
UNUSED_PROVIDER_BASE = "http://127.0.0.1:9/v1"


# --------------------------------------------------------------------------- #
# Live oracle helpers (the turn replay's shapes, minus the HTTP client)
# --------------------------------------------------------------------------- #


def _now_iso() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def _seed_member(db_conn, tag: str):
    """One user + workspace + membership; returns (user, workspace)."""
    seed_helpers.ensure_setup_done(db_conn)
    user = seed_helpers.user(db_conn, f"turn-{tag}")
    workspace = seed_helpers.workspace(db_conn, f"turn-{tag}", user["id"])
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
            (thread_id, str(workspace["id"]), str(user["id"]), "turn oracle", now, now),
        )
        cur.execute(
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
    db_conn.commit()
    return thread_id, turn_id, message_id


def _configure_llm(db_conn, user_id, base_url: str, model_name: str = MODEL_NAME):
    """Seed the BYOK row the Rust ``resolve_model`` reads.

    The ciphertext is Fernet under ``ASSISTANT_ENCRYPTION_KEY`` (the key the
    Rust worker is configured with), the same encoding the config PUT
    writes.
    """
    from cryptography.fernet import Fernet

    key = os.environ["ASSISTANT_ENCRYPTION_KEY"].encode("utf-8")
    token = Fernet(key).encrypt(API_KEY.encode("utf-8"))
    now = _now_iso()
    with db_conn.cursor() as cur:
        # `id` is a BigAutoField on this model (no explicit pk), so it is
        # left to the sequence.
        cur.execute(
            "INSERT INTO assistant_user_llm_config "
            "(user_id, provider_kind, base_url, model_name, api_key_encrypted, "
            "created_at, updated_at) "
            "VALUES (%s, %s, %s, %s, %s, %s, %s)",
            (
                str(user_id),
                PROVIDER_KIND,
                base_url,
                model_name,
                token,
                now,
                now,
            ),
        )
    db_conn.commit()


def _seed_mcp_server(db_conn, user_id, name, url):
    """One enabled MCP server row for the turn user (raw SQL: the hanging
    URL is a test double, not endpoint input)."""
    now = _now_iso()
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO assistant_mcp_server "
            "(id, user_id, name, url, auth_header_encrypted, is_enabled, "
            "created_at, updated_at) "
            "VALUES (%s, %s, %s, %s, NULL, TRUE, %s, %s)",
            (str(uuid.uuid4()), str(user_id), name, url, now, now),
        )
    db_conn.commit()


def _turn_row(db_conn, turn_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT * FROM assistant_turn WHERE id = %s", (turn_id,))
        return cur.fetchone()


def _thread_row(db_conn, thread_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT * FROM assistant_thread WHERE id = %s", (thread_id,))
        return cur.fetchone()


def _turn_events(db_conn, turn_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT kind, payload, message_id FROM assistant_event "
            "WHERE turn_id = %s ORDER BY seq",
            (turn_id,),
        )
        return cur.fetchall()


def _publish(turn_id, *, task=RUN_TURN, args=None):
    """Enqueue one turn for the Rust worker and return its celery id."""
    args = args if args is not None else [str(turn_id)]
    return rust_queue.publish(task, args=args)


def _hold_open(conn):
    """Hold one accepted connection open without ever answering.

    Request bytes are consumed and discarded; the client hangs until ITS
    read timeout — which the test keeps far past the worker's soft limit,
    so the soft limit deterministically wins.
    """
    conn.settimeout(3600)
    try:
        while True:
            if not conn.recv(65536):
                return
    except OSError:
        pass
    finally:
        try:
            conn.close()
        except OSError:
            pass


class _HangingMcpServer:
    """An MCP server that accepts the TCP connection but never answers."""

    def __init__(self):
        self.accepted = []
        self._sock = None
        self._thread = None

    def start(self):
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sock.bind(("127.0.0.1", 0))
        sock.listen(16)
        port = sock.getsockname()[1]
        self._sock = sock
        stub = self

        def serve():
            while True:
                try:
                    conn, _ = sock.accept()
                except OSError:
                    return
                stub.accepted.append(time.time())
                threading.Thread(target=_hold_open, args=(conn,), daemon=True).start()

        self._thread = threading.Thread(target=serve, daemon=True)
        self._thread.start()
        return f"http://127.0.0.1:{port}/mcp"

    def stop(self):
        if self._sock is not None:
            try:
                self._sock.close()
            except OSError:
                pass
            self._sock = None


# --------------------------------------------------------------------------- #
# Live replay: publish into rust_job_queue, diff the DB
# --------------------------------------------------------------------------- #


def test_mcp_entry_hang_fails_at_soft_limit(db_conn):
    """A turn hung in MCP toolset entry fails turn_timeout at the soft limit.

    The Rust worker under test boots with ASSISTANT_TURN_SOFT_LIMIT=60
    (see the MCP-entry replay job env): the ten MCP stubs accept the TCP
    connection but never answer, so toolset entry spans ~100s (sequential,
    ~10s per server) and the soft limit fires mid-entry. The turn fails
    with the turn_timeout code/detail before any provider request. The
    Django oracle is
    ``assistant/test_tasks_turn.py::test_mcp_entry_hang_fails_at_soft_limit``.
    """
    mcp = _HangingMcpServer()
    mcp_url = mcp.start()
    try:
        user, workspace = _seed_member(db_conn, "mcpentry")
        _configure_llm(db_conn, user["id"], UNUSED_PROVIDER_BASE)
        for n in range(MCP_ENTRY_HANGING_SERVERS):
            _seed_mcp_server(db_conn, user["id"], f"hang-{n}", mcp_url)
        thread_id, turn_id, _msg = _seed_turn_trio(db_conn, user, workspace)

        t0 = time.monotonic()
        celery_id = _publish(turn_id)
        row = wait_for(
            lambda: _turn_row(db_conn, turn_id)
            if _turn_row(db_conn, turn_id)["status"] == "failed"
            else None,
            what="mcp-entry turn failed",
            timeout=MCP_ENTRY_SOFT_LIMIT_SECS + 240,
        )
        elapsed = time.monotonic() - t0
        assert row["error_code"] == TIMEOUT_CODE
        assert row["error_detail"] == TIMEOUT_DETAIL
        assert elapsed >= MCP_ENTRY_SOFT_LIMIT_SECS - 5, (
            f"failed after {elapsed:.0f}s: too early to be the 60s soft limit"
        )
        # Past the 10s-per-server entries but nowhere near their ~100s
        # span: the soft limit won mid-entry (and not the sweep — its cutoff
        # is 390s on the default hard limit).
        assert elapsed < MCP_ENTRY_SOFT_LIMIT_SECS + 180, (
            f"failed after {elapsed:.0f}s: too late to be the 60s soft limit"
        )
        assert row["completed_at"] is not None
        assert _thread_row(db_conn, thread_id)["active_turn_id"] is None
        assert len(mcp.accepted) >= 2, "the turn entered MCP servers in turn"
        rust_queue.wait_for_settled(celery_id, what="mcp-entry turn settled")

        events = _turn_events(db_conn, turn_id)
        kinds = [e["kind"] for e in events]
        assert "turn_started" in kinds
        failed = [e for e in events if e["kind"] == "turn_failed"]
        assert len(failed) == 1, "exactly one failure event"
        assert failed[0]["payload"] == {
            "turn_id": turn_id,
            "error_code": TIMEOUT_CODE,
            "detail": TIMEOUT_DETAIL,
        }
        assert "turn_completed" not in kinds, "turn_completed followed turn_failed"
        assert "assistant_delta" not in kinds, "deltas are pruned on failure"
    finally:
        mcp.stop()
