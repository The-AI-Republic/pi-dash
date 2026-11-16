# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Replies cut off at the model's output limit are flagged (PIDASHCONV-1567).

Django main ``52df56b9e`` (PDASHOSS01-287) flags a reply the model cut short
at its output limit, two ways:

- the run completes but the reply is marked: ``payload.truncated`` on the
  turn's last assistant row, a re-sent ``message_completed`` carrying the
  refreshed envelope, and ``"truncated": true`` on ``turn_completed``
  (``tasks.py`` ``_was_cut_off`` / ``_mark_cut_off`` / ``_complete_turn``);
- the run dies from the limit (``IncompleteToolCall``, or
  ``UnexpectedModelBehavior`` mentioning the token limit) and fails with
  ``response_truncated`` plus the cut-off detail instead of ``internal``
  (``tasks.py`` ``_hit_output_limit``, after the provider classifier).

The Rust port had neither: ``complete_turn`` always emitted
``{"turn_id", "usage"}``, and the length/incomplete-tool-call errors fell
into ``internal``. This file pins the three black-box cases plus the
normal-finish control, against a stub OpenAI provider:

1. text chunks ending ``finish_reason: "length"`` → COMPLETED, row payload
   ``{"truncated": true}``, two ``message_completed`` events (plain, then
   flagged), ``turn_completed`` carries ``truncated: true``;
2. an empty response with ``finish_reason: "length"`` → FAILED with
   ``response_truncated`` and the cut-off detail;
3. a tool call with truncated JSON args and ``finish_reason: "length"`` on
   every request → FAILED with ``response_truncated`` and the detail;
4. (control) text ending ``finish_reason: "stop"`` → COMPLETED, row
   payload ``{}``, exactly one ``message_completed``, no ``truncated`` key.

Publish transport: the replay default seeds ``rust_job_queue`` for
``pidash-api worker`` (the CI replay job boots no Celery worker);
``PIDASH_CONTRACT_PUBLISH=celery`` reroutes to the Celery broker so the
same assertions run against live Django main as the oracle-validity gate.
The file is NOT in the worker-plane oracle job: that job runs
Django-on-rust-dev, which predates the truncation behavior until
PIDASHCONV-1055 merges main. The LLM config row is seeded straight into
``assistant_user_llm_config`` (Fernet ciphertext, the encoding the config
PUT writes) because the replay job boots no HTTP server.

Known non-asserted difference (pre-existing drift, out of scope): Django
retries invalid tool args (case 3 costs it 3 provider requests) while Rust
fails fast (1 request), so request counts and ``model_messages`` differ;
the terminal state, code/detail, row payload and event sequence match.
"""

from __future__ import annotations

import json
import os
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from psycopg.rows import dict_row

from _harness import celery_wire, rust_queue
from _harness import seed as seed_helpers
from _harness.db import wait_for_condition as wait_for

RUN_TURN = "assistant.run_turn"

API_KEY = "test-key-123"
PROVIDER_KIND = "openai_compatible"
MODEL_NAME = "oracle-stub"

TRUNCATED_CODE = "response_truncated"
TRUNCATED_DETAIL = "The reply was cut off because it reached the model's output limit."

CASE1_TEXT = "The answer starts here and"


def _now_iso() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def _seed_member(db_conn, tag: str):
    """One user + workspace + membership; returns (user, workspace)."""
    seed_helpers.ensure_setup_done(db_conn)
    user = seed_helpers.user(db_conn, f"trunc-{tag}")
    workspace = seed_helpers.workspace(db_conn, f"trunc-{tag}", user["id"])
    seed_helpers.workspace_member(db_conn, workspace["id"], user["id"], role=15)
    return user, workspace


def _seed_llm_config(db_conn, user_id, base_url):
    """Seed the BYOK row the worker's ``resolve_model`` reads.

    The ciphertext is Fernet under ``ASSISTANT_ENCRYPTION_KEY``, the same
    encoding the config PUT writes.
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
            (str(user_id), PROVIDER_KIND, base_url, MODEL_NAME, token, now, now),
        )


def _seed_turn_trio(db_conn, user, workspace):
    """Replicate the message-POST 202 rows (``views/messages.py``).

    Thread (with ``active_turn`` set) + user message (seq 1, completed) +
    turn pointing at the message. Returns (thread_id, turn_id).
    """
    thread_id, turn_id, message_id = (str(uuid.uuid4()) for _ in range(3))
    now = _now_iso()
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO assistant_thread "
            "(id, workspace_id, user_id, title, kind, is_archived, "
            "created_at, updated_at) "
            "VALUES (%s, %s, %s, %s, 'chat', FALSE, %s, %s)",
            (thread_id, str(workspace["id"]), str(user["id"]), "truncation oracle", now, now),
        )
        cur.execute(
            # model_used / error_code / error_detail are Django-side
            # defaults (""), not DB defaults: raw SQL must pass them.
            "INSERT INTO assistant_turn (id, thread_id, status, model_used, error_code, "
            "error_detail, created_at, started_at) "
            "VALUES (%s, %s, 'queued', '', '', '', %s, %s)",
            (turn_id, thread_id, now, None),
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
    return thread_id, turn_id


def _publish_turn(turn_id, broker_url):
    """Enqueue one turn on this run's worker.

    The replay default is ``rust_job_queue`` for ``pidash-api worker``;
    ``PIDASH_CONTRACT_PUBLISH=celery`` reroutes to the Celery broker for
    the live-Django oracle-validity run.
    """
    if os.environ.get("PIDASH_CONTRACT_PUBLISH") == "celery":
        celery_wire.publish(RUN_TURN, args=[turn_id], broker_url=broker_url)
    else:
        rust_queue.publish(RUN_TURN, args=[turn_id])


def _turn_row(db_conn, turn_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT * FROM assistant_turn WHERE id = %s", (turn_id,))
        return cur.fetchone()


def _assistant_rows(db_conn, turn_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT display_content, status, payload FROM assistant_message "
            "WHERE turn_id = %s AND kind = 'assistant' ORDER BY seq",
            (turn_id,),
        )
        return cur.fetchall()


def _turn_events(db_conn, turn_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT kind, payload FROM assistant_event WHERE turn_id = %s ORDER BY seq",
            (turn_id,),
        )
        return cur.fetchall()


def _run_case(db_conn, broker_url, tag, script):
    """Seed, publish and wait for one terminal turn; return (turn, rows, events)."""
    user, workspace = _seed_member(db_conn, tag)
    stub = _ScriptedStub(script)
    base = stub.start()
    try:
        _seed_llm_config(db_conn, user["id"], base)
        thread_id, turn_id = _seed_turn_trio(db_conn, user, workspace)
        _publish_turn(turn_id, broker_url)
        row = wait_for(
            lambda: next(
                (
                    r
                    for r in [_turn_row(db_conn, turn_id)]
                    if r["status"] in ("completed", "failed", "cancelled")
                ),
                None,
            ),
            what=f"{tag} terminal",
        )
        return row, _assistant_rows(db_conn, turn_id), _turn_events(db_conn, turn_id)
    finally:
        stub.stop()


# --------------------------------------------------------------------------- #
# Provider stub: scripted OpenAI chat-completions streams with finish control
# --------------------------------------------------------------------------- #


def _chunk(delta, finish):
    return (
        'data: {"id":"stub","object":"chat.completion.chunk",'
        '"created":0,"model":"oracle-stub","choices":[{"index":0,'
        '"delta":' + json.dumps(delta) + ',"finish_reason":' + json.dumps(finish) + "}]}\n\n"
    ).encode()


class _ScriptedStub:
    """Minimal OpenAI-compatible stub with a per-request stream script.

    ``script`` is a list of per-request chunk lists; each chunk is a
    ``(delta, finish_reason)`` pair. The last entry repeats, so a
    single-entry script answers every request identically (the truncated
    tool call must be served on every retry).
    """

    def __init__(self, script):
        self.script = script
        self.requests = []
        self._server = None

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
                index = len(stub.requests)
                stub.requests.append({"path": self.path, "stream": payload.get("stream")})
                chunks = stub.script[min(index, len(stub.script) - 1)]
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()
                for delta, finish in chunks:
                    self.wfile.write(_chunk(delta, finish))
                    self.wfile.flush()
                self.wfile.write(b"data: [DONE]\n\n")
                self.wfile.flush()

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self._server.daemon_threads = True
        port = self._server.server_address[1]
        thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        thread.start()
        return f"http://127.0.0.1:{port}/v1"

    def stop(self):
        if self._server is not None:
            self._server.shutdown()
            self._server.server_close()
            self._server = None


TEXT_THEN_LENGTH = [[({"content": CASE1_TEXT}, None), ({}, "length")]]
TEXT_THEN_STOP = [[({"content": CASE1_TEXT}, None), ({}, "stop")]]
EMPTY_THEN_LENGTH = [[({}, "length")]]
TRUNCATED_CALL = {
    "role": "assistant",
    "tool_calls": [
        {
            "index": 0,
            "id": "call_trunc_1",
            "type": "function",
            "function": {"name": "get_issue", "arguments": '{"issue_id": "abc'},
        }
    ],
}
TOOL_THEN_LENGTH = [[(TRUNCATED_CALL, None), ({}, "length")]]


# --------------------------------------------------------------------------- #
# The cases
# --------------------------------------------------------------------------- #


def _completed_event(events):
    return next(e for e in events if e["kind"] == "turn_completed")


def test_text_reply_cut_off_at_the_limit_is_marked(db_conn, broker_url):
    row, assistant_rows, events = _run_case(db_conn, broker_url, "text-length", TEXT_THEN_LENGTH)

    assert row["status"] == "completed"
    assert len(assistant_rows) == 1
    assert assistant_rows[0]["display_content"] == CASE1_TEXT
    assert assistant_rows[0]["status"] == "completed"
    assert assistant_rows[0]["payload"] == {"truncated": True}

    completed = [e for e in events if e["kind"] == "message_completed"]
    assert len(completed) == 2, [e["kind"] for e in events]
    assert completed[0]["payload"]["message"]["payload"] == {}
    assert completed[1]["payload"]["message"]["payload"] == {"truncated": True}

    assert _completed_event(events)["payload"]["truncated"] is True


def test_empty_reply_at_the_limit_fails_as_cut_off(db_conn, broker_url):
    row, assistant_rows, events = _run_case(db_conn, broker_url, "empty-length", EMPTY_THEN_LENGTH)

    assert row["status"] == "failed"
    assert row["error_code"] == TRUNCATED_CODE
    assert row["error_detail"] == TRUNCATED_DETAIL
    assert assistant_rows == []

    failed = next(e for e in events if e["kind"] == "turn_failed")
    assert failed["payload"]["error_code"] == TRUNCATED_CODE
    assert failed["payload"]["detail"] == TRUNCATED_DETAIL


def test_truncated_tool_call_at_the_limit_fails_as_cut_off(db_conn, broker_url):
    row, assistant_rows, events = _run_case(db_conn, broker_url, "tool-length", TOOL_THEN_LENGTH)

    assert row["status"] == "failed"
    assert row["error_code"] == TRUNCATED_CODE
    assert row["error_detail"] == TRUNCATED_DETAIL

    failed = next(e for e in events if e["kind"] == "turn_failed")
    assert failed["payload"]["error_code"] == TRUNCATED_CODE
    assert failed["payload"]["detail"] == TRUNCATED_DETAIL


def test_normal_finish_is_not_marked(db_conn, broker_url):
    row, assistant_rows, events = _run_case(db_conn, broker_url, "text-stop", TEXT_THEN_STOP)

    assert row["status"] == "completed"
    assert len(assistant_rows) == 1
    assert assistant_rows[0]["payload"] == {}

    assert len([e for e in events if e["kind"] == "message_completed"]) == 1
    assert "truncated" not in _completed_event(events)["payload"]
