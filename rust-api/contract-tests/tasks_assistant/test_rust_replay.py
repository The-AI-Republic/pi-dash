# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""assistant turn oracle vs the Rust worker (PIDASHCONV-863).

The Django oracle (``assistant/test_tasks_turn.py``) proves what the Python
plane does with ``assistant.run_turn`` + ``assistant.sweep_stale_turns``
Celery traffic. This suite proves the Rust worker's side for the turn task:
the name executes LOCALLY (it settles in ``rust_job_queue`` and the DB shows
the terminal turn writes), no Celery worker is involved, and the run is
driven end to end against a stub provider.

The replay seeds ``rust_job_queue`` rows with the exact Celery v2 payload
(``_harness/rust_queue.py``) — ``args=[str(turn_id)]``, the same wire the
message POST enqueues — and waits on the same Postgres table. The Rust
worker (``pidash-api worker``) must be running against the same
``DATABASE_URL``; the Django worker must be stopped.

Every path the turn walk covers is exercised: model resolution (missing
config, unreachable endpoint, resolved BYOK), the instructions + the 17
local tool schemas on the wire (toolset), the streamed text, usage
extraction, the terminal writes and the delta prune, and the cancel key.
BYOK ciphertext is Fernet under ``ASSISTANT_ENCRYPTION_KEY`` — the same
backend the Rust handlers decrypt with (``assistant/crypto.py``
``FernetBackend``), so the seeded row is what the worker reads.
"""

from __future__ import annotations

import json
import os
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from psycopg.rows import dict_row

from _harness import redis as redis_helper
from _harness import rust_queue
from _harness import seed as seed_helpers
from _harness.db import wait_for_condition as wait_for

RUN_TURN = "assistant.run_turn"
SWEEP = "assistant.sweep_stale_turns"

API_KEY = "test-key-123"
PROVIDER_KIND = "openai_compatible"
MODEL_NAME = "oracle-stub"


# --------------------------------------------------------------------------- #
# Live oracle helpers (the Django oracle's shapes, minus the HTTP client)
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


def _table_count(db_conn, table):
    with db_conn.cursor() as cur:
        cur.execute(f"SELECT count(*) FROM {table}")
        return cur.fetchone()[0]


def _publish(turn_id, *, task=RUN_TURN, args=None):
    """Enqueue one turn for the Rust worker and return its celery id."""
    args = args if args is not None else [str(turn_id)]
    return rust_queue.publish(task, args=args)


def _run_and_settle(db_conn, turn_id):
    """Publish one turn and wait for the Rust worker to ack the row."""
    celery_id = _publish(turn_id)
    rust_queue.wait_for_settled(celery_id, what="assistant turn settled")
    return celery_id


# --------------------------------------------------------------------------- #
# Provider stub: enough OpenAI chat-completions for a streaming turn
# --------------------------------------------------------------------------- #


class _StubProvider:
    """Minimal OpenAI-compatible stub for the BYOK openai_compatible branch.

    Serves ``POST /v1/chat/completions`` as an SSE stream of one chunk per
    ``interval`` seconds (``["Hello", " world"]`` by default) ending with
    ``data: [DONE]``. Every request body is recorded so the wire shape
    (tools, instructions, stream flag) can be asserted.
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
# Live oracle: publish into rust_job_queue, diff the DB, capture effects
# --------------------------------------------------------------------------- #


def test_unknown_turn_is_noop(db_conn):
    """A turn id with no eligible context changes nothing (``_load_context``)."""
    before = {
        table: _table_count(db_conn, table)
        for table in ("assistant_turn", "assistant_message", "assistant_event")
    }
    _run_and_settle(db_conn, uuid.uuid4())
    after = {
        table: _table_count(db_conn, table)
        for table in ("assistant_turn", "assistant_message", "assistant_event")
    }
    assert before == after


def test_missing_config_fails_turn(db_conn):
    """No BYOK config: model resolution fails the turn (``llm_config_missing``)."""
    user, workspace = _seed_member(db_conn, "noconfig")
    _thread_id, turn_id, _msg = _seed_turn_trio(db_conn, user, workspace)

    _run_and_settle(db_conn, turn_id)
    row = wait_for(
        lambda: _turn_row(db_conn, turn_id)
        if _turn_row(db_conn, turn_id)["status"] == "failed"
        else None,
        what="missing-config turn failed",
    )
    assert row["error_code"] == "llm_config_missing"
    assert row["error_detail"] == "No LLM provider is configured for this user."
    assert row["completed_at"] is not None
    assert row["started_at"] is not None, "the claim stamps started_at first"

    kinds = [e["kind"] for e in _turn_events(db_conn, turn_id)]
    assert "turn_started" in kinds
    assert "turn_failed" in kinds


def test_unreachable_provider_fails_turn(db_conn):
    """A dead endpoint classifies as ``provider_unreachable`` (``_classify_error``)."""
    user, workspace = _seed_member(db_conn, "unreachable")
    # A port that is bound-and-dropped (never a listener): connect fails fast.
    _configure_llm(db_conn, user["id"], "http://127.0.0.1:9/v1")
    _thread_id, turn_id, _msg = _seed_turn_trio(db_conn, user, workspace)

    _run_and_settle(db_conn, turn_id)
    row = wait_for(
        lambda: _turn_row(db_conn, turn_id)
        if _turn_row(db_conn, turn_id)["status"] == "failed"
        else None,
        what="unreachable-provider turn failed",
    )
    assert row["error_code"] == "provider_unreachable"
    assert row["error_detail"] == "Could not reach the configured provider endpoint."


def test_complete_turn_streams_and_prunes(db_conn):
    """Stub provider: deltas stream, the turn completes, deltas are pruned.

    The stub records the request body, so the wire is asserted too: the
    instructions travel as leading system messages and the 17 local tool
    schemas are offered (the model/toolset/instructions paths).
    """
    stub = _StubProvider()
    base = stub.start()
    try:
        user, workspace = _seed_member(db_conn, "complete")
        _configure_llm(db_conn, user["id"], base)
        thread_id, turn_id, _msg = _seed_turn_trio(db_conn, user, workspace)

        _run_and_settle(db_conn, turn_id)
        row = wait_for(
            lambda: _turn_row(db_conn, turn_id)
            if _turn_row(db_conn, turn_id)["status"] == "completed"
            else None,
            what="stub turn completed",
        )
        assert row["model_used"] == "openai_compatible:oracle-stub"
        assert isinstance(row["model_messages"], list) and row["model_messages"], (
            "completed content is stored for replay"
        )
        assert set(row["usage"]) >= {
            "input_tokens",
            "output_tokens",
            "total_tokens",
            "requests",
            "tool_calls",
        }
        assert _thread_row(db_conn, thread_id)["active_turn_id"] is None

        with db_conn.cursor(row_factory=dict_row) as cur:
            cur.execute(
                "SELECT display_content, status FROM assistant_message "
                "WHERE turn_id = %s AND kind = 'assistant' ORDER BY seq",
                (turn_id,),
            )
            assistant_rows = cur.fetchall()
        assert assistant_rows, "the stream wrote at least one assistant row"
        assert "".join(r["display_content"] for r in assistant_rows) == "Hello world"
        assert all(r["status"] == "completed" for r in assistant_rows)

        kinds = [e["kind"] for e in _turn_events(db_conn, turn_id)]
        assert "turn_started" in kinds
        assert "message_created" in kinds
        assert "message_completed" in kinds
        assert "turn_completed" in kinds
        assert "assistant_delta" not in kinds, "deltas are pruned on completion"
        completed = next(
            e for e in _turn_events(db_conn, turn_id) if e["kind"] == "turn_completed"
        )
        assert completed["payload"]["usage"] == row["usage"]

        # The wire the worker sent: one streaming completion request with the
        # 17 local tools and the instruction system messages.
        assert stub.requests, "the provider was called"
        body = stub.requests[0]["body"]
        assert body["stream"] is True
        assert body["model"] == MODEL_NAME
        tools = body["tools"]
        names = [t["function"]["name"] for t in tools]
        assert len(names) == 17, f"the 17 local tools, got {names!r}"
        assert names[0] == "create_comment"  # registration order
        system = [m for m in body["messages"] if m["role"] == "system"]
        assert len(system) == 2, "base instructions + loop appendix"
        assert "Pi Dash" in system[0]["content"]
        user_messages = [
            m
            for m in body["messages"]
            if m["role"] == "user" and m["content"] == "do a thing"
        ]
        assert user_messages, "the user prompt is on the wire"
    finally:
        stub.stop()


def test_cancel_mid_stream_cancels_turn(db_conn):
    """Cancel key set mid-stream: the turn cancels, the open row closes as
    cancelled, deltas are pruned (``views/cancel.py`` key contract)."""
    stub = _StubProvider(chunks=tuple(f"chunk-{i}" for i in range(30)), interval=0.5)
    base = stub.start()
    try:
        user, workspace = _seed_member(db_conn, "cancel")
        _configure_llm(db_conn, user["id"], base)
        _thread_id, turn_id, _msg = _seed_turn_trio(db_conn, user, workspace)

        celery_id = _publish(turn_id)
        # Wait for a streamed delta (an assistant row is open and deltas are
        # flowing), then set the cancel key the endpoint writes. The worker
        # observes the key when the next event arrives.
        wait_for(
            lambda: [
                e
                for e in _turn_events(db_conn, turn_id)
                if e["kind"] == "assistant_delta"
            ]
            or None,
            what="first streamed delta",
        )
        rdb = redis_helper.client()
        rdb.set(f"assistant:cancel:{turn_id}", "1", ex=300)

        row = wait_for(
            lambda: _turn_row(db_conn, turn_id)
            if _turn_row(db_conn, turn_id)["status"] == "cancelled"
            else None,
            what="turn cancelled",
        )
        assert row["completed_at"] is not None
        assert row["error_code"] in (None, ""), "cancel writes no error code"
        rust_queue.wait_for_settled(celery_id, what="cancelled turn settled")

        with db_conn.cursor(row_factory=dict_row) as cur:
            cur.execute(
                "SELECT status FROM assistant_message WHERE turn_id = %s AND kind = 'assistant'",
                (turn_id,),
            )
            streamed = cur.fetchall()
        assert streamed and all(m["status"] == "cancelled" for m in streamed)

        kinds = [e["kind"] for e in _turn_events(db_conn, turn_id)]
        assert "turn_cancelled" in kinds
        assert "assistant_delta" not in kinds, "deltas are pruned on cancel"

        assert rdb.get(f"assistant:cancel:{turn_id}") is not None
        rdb.close()
    finally:
        stub.stop()
