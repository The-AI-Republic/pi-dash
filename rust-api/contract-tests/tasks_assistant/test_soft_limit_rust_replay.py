# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Configured soft limit survival vs the Rust worker (PIDASHCONV-1458).

The Django oracle is
``assistant/test_tasks_turn.py::test_turn_honors_configured_soft_limit``:
with ``ASSISTANT_TURN_SOFT_LIMIT=600`` a turn streaming for ~400s — past
the 300s default kill point — completes instead of failing with
``turn_timeout``. This file proves the Rust side of the same contract:
``assistant.run_turn`` executes LOCALLY in the Rust worker (``worker()``
wired the handler in PIDASHCONV-863), so the replay seeds a
``rust_job_queue`` row with the exact Celery v2 payload
(``_harness/rust_queue.py``) and diffs the database. The Rust worker
(``pidash-api worker``) must be running against the same ``DATABASE_URL``
with ``ASSISTANT_TURN_SOFT_LIMIT=600`` (see the soft-limit replay job
env; ``ASSISTANT_TURN_HARD_LIMIT=900`` there keeps the scheduler sweep's
960s cutoff clear of the ~400s turn); no Celery worker, broker or HTTP
server is involved.

Regression pinned here: the turn runner computed its stream deadline as
drive-start + a hard-coded 300 (``TURN_SOFT_LIMIT_SECS``), ignoring the
configured ``settings.assistant.turn_soft_limit``. Pre-fix the ~400s turn
fails at ~300s with ``turn_timeout`` and this test fails; post-fix the
turn completes.

A dedicated job boots the worker (the shared replay job must keep the
300 default for ``test_slow_stream_fails_at_soft_limit``), so this file
carries its own copy of the replay helpers (the
``test_sweep_rust_replay.py`` shape), not an import of the
default-env suite.
"""

from __future__ import annotations

import json
import os
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from psycopg.rows import dict_row

from _harness import rust_queue
from _harness import seed as seed_helpers
from _harness.db import wait_for_condition as wait_for

RUN_TURN = "assistant.run_turn"

API_KEY = "test-key-123"
PROVIDER_KIND = "openai_compatible"
MODEL_NAME = "oracle-stub"

#: Configured soft limit survival (PIDASHCONV-1458): the stub streams one
#: chunk every 20s for ~400s — past the 300s default kill point, inside the
#: 600s configured soft limit and the 600s per-request client timeout.
SOFT_STREAM_CHUNKS = 20
SOFT_STREAM_INTERVAL = 20.0
DEFAULT_SOFT_LIMIT_SECS = 300


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


# --------------------------------------------------------------------------- #
# Provider stub: enough OpenAI chat-completions for a streaming turn
# --------------------------------------------------------------------------- #


class _StubProvider:
    """Minimal OpenAI-compatible stub for the BYOK openai_compatible branch.

    Serves ``POST /v1/chat/completions`` as an SSE stream of one chunk per
    ``interval`` seconds ending with ``data: [DONE]``.
    """

    def __init__(self, chunks=("Hello", " world"), interval=0.2):
        self.chunks = list(chunks)
        self.interval = interval
        self.requests = []
        self._server = None
        self._thread = None

    def start(self):
        stub = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.0"

            def log_message(self, *args):
                pass

            def do_POST(self):
                length = int(self.headers.get("Content-Length", 0))
                body = self.rfile.read(length) if length else b""
                try:
                    payload = json.loads(body or b"{}")
                except ValueError:
                    payload = {}
                stub.requests.append({"path": self.path, "body": payload})
                if self.path.endswith("/chat/completions") and payload.get("stream"):
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream")
                    self.end_headers()
                    for chunk in stub.chunks:
                        time.sleep(stub.interval)
                        self.wfile.write(
                            (
                                'data: {"id":"stub","object":"chat.completion.chunk",'
                                '"created":0,"model":"oracle-stub","choices":[{"index":0,'
                                '"delta":{"content":'
                                + json.dumps(chunk)
                                + '},"finish_reason":null}]}\n\n'
                            ).encode()
                        )
                        self.wfile.flush()
                    self.wfile.write(b"data: [DONE]\n\n")
                    self.wfile.flush()
                else:
                    content = json.dumps(
                        {
                            "id": "stub",
                            "object": "chat.completion",
                            "created": 0,
                            "model": "oracle-stub",
                            "choices": [
                                {
                                    "index": 0,
                                    "message": {"role": "assistant", "content": "stub"},
                                    "finish_reason": "stop",
                                }
                            ],
                            "usage": {
                                "prompt_tokens": 1,
                                "completion_tokens": 1,
                                "total_tokens": 2,
                            },
                        }
                    ).encode()
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(content)))
                    self.end_headers()
                    self.wfile.write(content)

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self._server.daemon_threads = True
        port = self._server.server_address[1]
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()
        return f"http://127.0.0.1:{port}/v1"

    def stop(self):
        if self._server is not None:
            self._server.shutdown()
            self._server.server_close()
            self._server = None


# --------------------------------------------------------------------------- #
# Live replay: publish into rust_job_queue, diff the DB
# --------------------------------------------------------------------------- #


def test_turn_honors_configured_soft_limit(db_conn):
    """A turn streaming past 300s but inside ASSISTANT_TURN_SOFT_LIMIT completes.

    The Rust worker under test boots with ASSISTANT_TURN_SOFT_LIMIT=600
    (see the soft-limit replay job env): the stub streams for ~400s — past
    the default 300s kill point — and the turn completes instead of failing
    with ``turn_timeout``. Pre-fix the runner hard-coded 300 and the turn
    failed at ~300s, so this test fails. The Django oracle is
    ``assistant/test_tasks_turn.py::test_turn_honors_configured_soft_limit``.
    """
    stub = _StubProvider(
        chunks=tuple(f"long-{i}" for i in range(SOFT_STREAM_CHUNKS)),
        interval=SOFT_STREAM_INTERVAL,
    )
    base = stub.start()
    try:
        user, workspace = _seed_member(db_conn, "softlimit")
        _configure_llm(db_conn, user["id"], base)
        thread_id, turn_id, _msg = _seed_turn_trio(db_conn, user, workspace)

        t0 = time.monotonic()
        celery_id = _publish(turn_id)
        row = wait_for(
            lambda: _turn_row(db_conn, turn_id)
            if _turn_row(db_conn, turn_id)["status"] == "completed"
            else None,
            what="long turn completed",
            timeout=SOFT_STREAM_CHUNKS * SOFT_STREAM_INTERVAL + 120,
        )
        elapsed = time.monotonic() - t0
        assert elapsed > DEFAULT_SOFT_LIMIT_SECS, (
            f"completed after {elapsed:.0f}s: never streamed past the 300s default kill point"
        )
        assert row["completed_at"] is not None
        assert row["error_code"] in (None, ""), "completion writes no error code"
        assert _thread_row(db_conn, thread_id)["active_turn_id"] is None
        rust_queue.wait_for_settled(celery_id, what="long turn settled")

        with db_conn.cursor(row_factory=dict_row) as cur:
            cur.execute(
                "SELECT display_content, status FROM assistant_message "
                "WHERE turn_id = %s AND kind = 'assistant' ORDER BY seq",
                (turn_id,),
            )
            assistant_rows = cur.fetchall()
        assert assistant_rows, "the stream wrote at least one assistant row"
        assert "".join(r["display_content"] for r in assistant_rows) == "".join(
            f"long-{i}" for i in range(SOFT_STREAM_CHUNKS)
        )
        assert all(r["status"] == "completed" for r in assistant_rows)

        kinds = [e["kind"] for e in _turn_events(db_conn, turn_id)]
        assert "turn_started" in kinds
        assert "turn_completed" in kinds
        assert "turn_failed" not in kinds, "no failure inside the configured limit"
        assert "assistant_delta" not in kinds, "deltas are pruned on completion"
    finally:
        stub.stop()
