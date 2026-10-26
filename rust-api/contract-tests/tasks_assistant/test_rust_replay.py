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
extraction, the terminal writes and the delta prune, the cancel key, and a
mid-stream seam fault failing the turn at once (PIDASHCONV-1254, via a
turn-scoped trigger on the delta writes).
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
from _harness.db import insert_row
from _harness.db import wait_for_condition as wait_for

RUN_TURN = "assistant.run_turn"
SWEEP = "assistant.sweep_stale_turns"

API_KEY = "test-key-123"
PROVIDER_KIND = "openai_compatible"
MODEL_NAME = "oracle-stub"

#: Slow-stream turn timeout (PIDASHCONV-1253): the stub streams one chunk
#: every 20s for ~460s — past the 300s soft limit and the 390s sweep cutoff,
#: inside the 600s per-request client timeout.
SLOW_CHUNKS = 23
SLOW_INTERVAL = 20.0
SOFT_LIMIT_SECS = 300
SWEEP_CUTOFF_SECS = 390

TIMEOUT_CODE = "turn_timeout"
TIMEOUT_DETAIL = "The assistant took too long to respond."


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

    ``tool_call`` (``(name, args)``) makes the *first* streaming request emit
    a single tool call instead of text (PIDASHCONV-1250): one delta carrying
    the complete call, closed by a ``tool_calls`` finish. Later requests
    stream ``chunks`` as usual, so the turn completes after the tool runs.
    """

    def __init__(self, chunks=("Hello", " world"), interval=0.2, tool_call=None):
        self.chunks = list(chunks)
        self.interval = interval
        self.tool_call = tool_call
        self._tool_call_sent = False
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
                    if stub.tool_call is not None and not stub._tool_call_sent:
                        stub._tool_call_sent = True
                        name, args = stub.tool_call
                        call_delta = {
                            "role": "assistant",
                            "tool_calls": [
                                {
                                    "index": 0,
                                    "id": "call_stateless_1",
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": json.dumps(args),
                                    },
                                }
                            ],
                        }
                        for delta, finish in ((call_delta, None), ({}, "tool_calls")):
                            time.sleep(stub.interval)
                            self.wfile.write(
                                (
                                    'data: {"id":"stub","object":"chat.completion.chunk",'
                                    '"created":0,"model":"oracle-stub","choices":[{"index":0,'
                                    '"delta":'
                                    + json.dumps(delta)
                                    + ',"finish_reason":'
                                    + json.dumps(finish)
                                    + "}]}\n\n"
                                ).encode()
                            )
                            self.wfile.flush()
                        self.wfile.write(b"data: [DONE]\n\n")
                        self.wfile.flush()
                        return
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


def test_slow_stream_fails_at_soft_limit(db_conn):
    """A turn streaming past 300s fails with ``turn_timeout``, and no
    ``turn_completed`` follows (PIDASHCONV-1253).

    The stub streams for ~460s, past the 390s sweep cutoff: the failure must
    land at the soft limit (300s, not the sweep), carry Django's detail, and
    stay failed after the stream ends.
    """
    stub = _StubProvider(
        chunks=tuple(f"slow-{i}" for i in range(SLOW_CHUNKS)), interval=SLOW_INTERVAL
    )
    base = stub.start()
    try:
        user, workspace = _seed_member(db_conn, "slow")
        _configure_llm(db_conn, user["id"], base)
        thread_id, turn_id, _msg = _seed_turn_trio(db_conn, user, workspace)

        t0 = time.monotonic()
        celery_id = _publish(turn_id)
        row = wait_for(
            lambda: _turn_row(db_conn, turn_id)
            if _turn_row(db_conn, turn_id)["status"] == "failed"
            else None,
            what="slow turn failed",
            timeout=SWEEP_CUTOFF_SECS + 60,
        )
        elapsed = time.monotonic() - t0
        assert row["error_code"] == TIMEOUT_CODE
        assert row["error_detail"] == TIMEOUT_DETAIL
        assert elapsed >= SOFT_LIMIT_SECS - 5, (
            f"failed after {elapsed:.0f}s: too early to be the soft limit"
        )
        assert elapsed < SWEEP_CUTOFF_SECS - 5, (
            f"failed after {elapsed:.0f}s: the sweep won, not the soft limit"
        )
        assert row["completed_at"] is not None
        assert _thread_row(db_conn, thread_id)["active_turn_id"] is None
        rust_queue.wait_for_settled(celery_id, what="timed-out turn settled")

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

        # Hold until the stream ends plus a settle margin, then prove no
        # turn_completed followed the failure.
        hold = (SLOW_CHUNKS * SLOW_INTERVAL + 60) - (time.monotonic() - t0)
        if hold > 0:
            time.sleep(hold)
        kinds = [e["kind"] for e in _turn_events(db_conn, turn_id)]
        assert "turn_completed" not in kinds, "turn_completed followed turn_failed"
        assert _turn_row(db_conn, turn_id)["status"] == "failed"
        assert "assistant_delta" not in kinds, "deltas are pruned on failure"
    finally:
        stub.stop()


# --------------------------------------------------------------------------- #
# Stateless-issue tool visibility (PIDASHCONV-1250)
# --------------------------------------------------------------------------- #


def _seed_stateless_issue(db_conn, user, workspace):
    """Project (the turn user is an admin member of) + one ``state_id`` NULL issue.

    Column sets mirror ``issue_chain`` (every NOT NULL projects/issues column
    without a database default); the issue simply carries no state.
    """
    project = seed_helpers.project(db_conn, workspace["id"], "Stateless Tools")
    seed_helpers.project_member(
        db_conn, project["id"], workspace["id"], user["id"], role=20
    )
    issue = insert_row(
        db_conn,
        "issues",
        {
            "workspace_id": str(workspace["id"]),
            "project_id": str(project["id"]),
            "state_id": None,
            "name": "stateless tool-target issue",
            "description_json": {},
            "description_html": "<p></p>",
            "priority": "none",
            "complexity_score": 0,
            "sequence_id": 1,
            "is_draft": False,
            "sort_order": 65535,
            "git_work_branch": "",
            "workpad": "",
        },
    )
    return project, issue


def _tool_returns(model_messages, tool_name):
    """``tool-return`` parts for ``tool_name`` across the stored messages."""
    found = []
    for message in model_messages or []:
        for part in (message.get("parts") or []):
            if part.get("part_kind") == "tool-return" and part.get("tool_name") == tool_name:
                found.append(part)
    return found


def test_turn_tools_see_stateless_issue(db_conn):
    """A turn calling ``get_issue`` on a ``state_id`` NULL issue gets it back.

    Django renders the ``issue_objects`` triage exclusion null-safely (``NOT
    (group = 'triage' AND group IS NOT NULL)``), so stateless rows stay
    visible to every read tool. The Django oracle of this test lives in
    ``assistant/test_tasks_turn.py``.
    """
    user, workspace = _seed_member(db_conn, "stateless")
    _project, issue = _seed_stateless_issue(db_conn, user, workspace)
    stub = _StubProvider(
        chunks=("done",),
        interval=0.1,
        tool_call=("get_issue", {"issue_id": str(issue["id"])}),
    )
    base = stub.start()
    try:
        _configure_llm(db_conn, user["id"], base)
        _thread_id, turn_id, _msg = _seed_turn_trio(db_conn, user, workspace)

        _run_and_settle(db_conn, turn_id)
        row = wait_for(
            lambda: _turn_row(db_conn, turn_id)
            if _turn_row(db_conn, turn_id)["status"] == "completed"
            else None,
            what="stateless-issue turn completed",
        )
        returns = _tool_returns(row["model_messages"], "get_issue")
        assert returns, (
            "no get_issue tool-return in model_messages: "
            f"{json.dumps(row['model_messages'])[:2000]}"
        )
        content = returns[0]["content"]
        if isinstance(content, str):
            content = json.loads(content)
        assert content["id"] == str(issue["id"]), (
            f"get_issue missed the stateless issue: {str(content)[:500]}"
        )
    finally:
        stub.stop()


# --------------------------------------------------------------------------- #
# Seam fault injection: a trigger fails one turn's delta writes (PIDASHCONV-1254)
# --------------------------------------------------------------------------- #

#: The trigger's RAISE text. It matches no ``_classify_error`` marker, so a
#: mid-stream seam fault classifies ``internal`` on both backends (mirrors
#: ``assistant/test_turn_errors.py``; modules stay independent).
SEAM_FAULT_MESSAGE = "seam fault injection: assistant_delta blocked"


def _install_delta_fault(db_conn, tag: str, turn_id: str):
    """Fail this turn's ``assistant_delta`` writes with a turn-scoped trigger.

    ``BEFORE INSERT ON assistant_event ... WHEN (kind, turn_id)``: only this
    turn's deltas raise; every other write (other turns, other kinds — in
    particular the ``turn_failed`` event the failure path emits) passes.
    Names carry the unique tag so parallel runs never share a trigger. Both
    interpolated values are generated hex/UUIDs, never user input.
    """
    fn = f"seam_fault_{tag}"
    trg = f"seam_fault_{tag}_trg"
    with db_conn.cursor() as cur:
        cur.execute(
            f"CREATE OR REPLACE FUNCTION {fn}() RETURNS trigger LANGUAGE plpgsql AS "
            f"$$ BEGIN RAISE EXCEPTION '{SEAM_FAULT_MESSAGE}'; END $$"
        )
        cur.execute(f"DROP TRIGGER IF EXISTS {trg} ON assistant_event")
        cur.execute(
            f"CREATE TRIGGER {trg} BEFORE INSERT ON assistant_event FOR EACH ROW "
            f"WHEN (NEW.kind = 'assistant_delta' AND NEW.turn_id = '{turn_id}') "
            f"EXECUTE FUNCTION {fn}()"
        )
    db_conn.commit()
    return fn, trg


def _remove_delta_fault(db_conn, fn: str, trg: str):
    with db_conn.cursor() as cur:
        cur.execute(f"DROP TRIGGER IF EXISTS {trg} ON assistant_event")
        cur.execute(f"DROP FUNCTION IF EXISTS {fn}()")
    db_conn.commit()


def test_midstream_seam_failure_fails_turn_at_once(db_conn):
    """A DB error from a mid-stream row write fails the turn at once.

    The trigger fails the first ``assistant_delta`` INSERT — the black-box
    shape of ``emit_delta`` raising out of the stream loop. The driver must
    take the same path as a provider error (report, fail the open row,
    classify ``internal``, fail the turn with ``turn_failed``) — immediately,
    not at the 390s sweep. Before the PIDASHCONV-1254 fix the turn stayed
    RUNNING and this test timed out waiting for ``failed``.
    """
    stub = _StubProvider(chunks=("Hello",))
    base = stub.start()
    tag = uuid.uuid4().hex[:8]
    fn = trg = None
    try:
        user, workspace = _seed_member(db_conn, f"seam{tag}")
        _configure_llm(db_conn, user["id"], base)
        thread_id, turn_id, _msg = _seed_turn_trio(db_conn, user, workspace)
        fn, trg = _install_delta_fault(db_conn, tag, turn_id)

        t0 = time.monotonic()
        celery_id = _publish(turn_id)
        row = wait_for(
            lambda: _turn_row(db_conn, turn_id)
            if _turn_row(db_conn, turn_id)["status"] == "failed"
            else None,
            what="seam-fault turn failed",
        )
        elapsed = time.monotonic() - t0
        assert elapsed < SWEEP_CUTOFF_SECS, (
            f"failed after {elapsed:.0f}s: the sweep won, not the run"
        )
        rust_queue.wait_for_settled(celery_id, what="seam-fault turn settled")

        def _failure_writes_complete():
            kinds = [e["kind"] for e in _turn_events(db_conn, turn_id)]
            if "turn_failed" in kinds and "assistant_delta" not in kinds:
                return kinds
            return None

        kinds = wait_for(_failure_writes_complete, what="seam-fault failure writes")
        assert row["error_code"] == "internal"
        assert row["error_detail"] == "The assistant hit an unexpected error."
        assert row["completed_at"] is not None
        assert row["started_at"] is not None

        assert _thread_row(db_conn, thread_id)["active_turn_id"] is None

        with db_conn.cursor(row_factory=dict_row) as cur:
            cur.execute(
                "SELECT display_content, status FROM assistant_message "
                "WHERE turn_id = %s AND kind = 'assistant' ORDER BY seq",
                (turn_id,),
            )
            streamed = cur.fetchall()
        assert streamed, "the streamed chunk wrote an assistant row"
        assert "".join(r["display_content"] for r in streamed) == "Hello"
        assert all(r["status"] == "failed" for r in streamed)

        with db_conn.cursor(row_factory=dict_row) as cur:
            cur.execute(
                "SELECT display_content, status FROM assistant_message "
                "WHERE turn_id = %s AND kind = 'error'",
                (turn_id,),
            )
            err = cur.fetchone()
        assert err is not None and err["status"] == "failed"
        assert err["display_content"] == row["error_detail"]

        failed = next(e for e in _turn_events(db_conn, turn_id) if e["kind"] == "turn_failed")
        assert failed["payload"] == {
            "turn_id": turn_id,
            "error_code": "internal",
            "detail": "The assistant hit an unexpected error.",
        }
        assert "assistant_delta" not in kinds, "deltas are pruned on failure"
    finally:
        if fn is not None:
            _remove_delta_fault(db_conn, fn, trg)
        stub.stop()
# create_issue post-save orchestration (PIDASHCONV-1251)
# --------------------------------------------------------------------------- #

TICKING_STATE = "In Progress"
TICKING_GROUP = "started"

BOUNCE_STRIPPED = (
    "Agent run skipped \u2014 no eligible runner."
    "No runner is registered in this pod that can serve this issue. "
    "Add a runner under your account, or assign this issue to a workspace "
    "member whose runner is registered here."
)


def _seed_ticking_project(db_conn, user, workspace):
    """Project with ticking enabled, one ``In Progress`` state, member access.

    Returns (project_id, state_id). The local-runner executor is explicit:
    the seed default (``""``) is not a runnable executor.
    """
    seed = seed_helpers.Seed(db_conn)
    project_id = seed.project(workspace["id"])
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE projects SET agent_ticking_enabled=TRUE, "
            "agent_default_interval_seconds=10800, "
            "agent_review_default_interval_seconds=10800, "
            "agent_test_default_interval_seconds=10800, "
            "agent_default_max_ticks=20, default_agent_executor='local_runner' "
            "WHERE id = %s",
            (project_id,),
        )
    state_id = seed.state(project_id, workspace["id"], name=TICKING_STATE, group=TICKING_GROUP)
    seed.project_member(project_id, workspace["id"], user["id"], role=15)
    db_conn.commit()
    return project_id, state_id


def _seed_pod(db_conn, workspace, project_id):
    pod_id = seed_helpers.Seed(db_conn).pod(workspace["id"], project_id)
    db_conn.commit()
    return pod_id


def _seed_runner(db_conn, user, workspace, pod_id):
    runner_id = seed_helpers.Seed(db_conn).runner(user["id"], workspace["id"], pod_id)
    db_conn.commit()
    return runner_id


def _issue_rows(db_conn, project_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT * FROM issues WHERE project_id = %s AND deleted_at IS NULL "
            "ORDER BY created_at",
            (project_id,),
        )
        return cur.fetchall()


def _ticker_rows(db_conn, issue_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT * FROM issue_agent_ticker WHERE issue_id = %s", (issue_id,))
        return cur.fetchall()


def _run_rows(db_conn, issue_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT * FROM agent_run WHERE work_item_id = %s ORDER BY created_at",
            (issue_id,),
        )
        return cur.fetchall()


def _comment_rows(db_conn, issue_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT * FROM issue_comments WHERE issue_id = %s ORDER BY created_at",
            (issue_id,),
        )
        return cur.fetchall()


def _run_event_count(db_conn, run_id):
    with db_conn.cursor() as cur:
        cur.execute("SELECT count(*) FROM agent_run_event WHERE agent_run_id = %s", (run_id,))
        return cur.fetchone()[0]


class _ScriptedStub:
    """OpenAI stub whose first reply is one tool call (SSE, the
    ``tool_local.json`` capture shape), every later reply text.

    ``tool_args`` is the tool's argument object; the call id is fixed
    (``call_1``) so the exchange is deterministic.
    """

    def __init__(self, tool_name: str, tool_args: dict, text: str = "Done"):
        self.tool_name = tool_name
        self.tool_args = tool_args
        self.text = text
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
                stub.requests.append(payload)
                first = len(stub.requests) == 1
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()
                if first:
                    self.wfile.write(
                        (
                            'data: {"id":"stub","object":"chat.completion.chunk",'
                            '"created":0,"model":"oracle-stub","choices":[{"index":0,'
                            '"delta":{"tool_calls":[{"index":0,"id":"call_1",'
                            '"type":"function","function":{"name":'
                            + json.dumps(stub.tool_name)
                            + ',"arguments":'
                            + json.dumps(json.dumps(stub.tool_args))
                            + '}}]},"finish_reason":null}]}\n\n'
                        ).encode()
                    )
                    self.wfile.write(
                        b'data: {"id":"stub","object":"chat.completion.chunk",'
                        b'"created":0,"model":"oracle-stub","choices":[{"index":0,'
                        b'"delta":{},"finish_reason":"tool_calls"}]}\n\n'
                    )
                else:
                    self.wfile.write(
                        (
                            'data: {"id":"stub","object":"chat.completion.chunk",'
                            '"created":0,"model":"oracle-stub","choices":[{"index":0,'
                            '"delta":{"content":' + json.dumps(stub.text) + "},"
                            '"finish_reason":null}]}\n\n'
                        ).encode()
                    )
                    self.wfile.write(
                        b'data: {"id":"stub","object":"chat.completion.chunk",'
                        b'"created":0,"model":"oracle-stub","choices":[{"index":0,'
                        b'"delta":{},"finish_reason":"stop"}]}\n\n'
                    )
                self.wfile.write(b"data: [DONE]\n\n")
                self.wfile.flush()

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


def _terminal_turn(db_conn, turn_id):
    row = _turn_row(db_conn, turn_id)
    db_conn.commit()
    if row and row["status"] in ("completed", "failed", "cancelled"):
        return row
    return None


def _drive_create_issue(db_conn, tag, *, pod=False, runner=False, backlog=False):
    """Seed, drive one ``create_issue`` tool call, wait for the terminal
    turn. Returns (user, project_id, state_id, pod_id, turn_id, requests),
    where requests are the provider stub's captured payloads (the
    follow-up request carries the tool result). ``backlog`` seeds one
    Backlog state so the bounce has a target (PIDASHCONV-1444)."""
    user, workspace = _seed_member(db_conn, f"create-{tag}")
    project_id, state_id = _seed_ticking_project(db_conn, user, workspace)
    if backlog:
        seed_helpers.Seed(db_conn).state(
            project_id, workspace["id"], name="Backlog", group="backlog"
        )
        db_conn.commit()
    pod_id = _seed_pod(db_conn, workspace, project_id) if pod else None
    if runner:
        assert pod_id is not None
        _seed_runner(db_conn, user, workspace, pod_id)
    _thread_id, turn_id, _message_id = _seed_turn_trio(db_conn, user, workspace)
    stub = _ScriptedStub(
        "create_issue",
        {
            "project_id": str(project_id),
            "name": f"transition probe {tag}",
            "state_id": str(state_id),
        },
    )
    base = stub.start()
    try:
        _configure_llm(db_conn, user["id"], base)
        _publish(turn_id)
        row = wait_for(
            lambda: _terminal_turn(db_conn, turn_id),
            what="create_issue turn terminal",
        )
        assert row["status"] == "completed", (
            f"turn {row['status']}: {row['error_code']} {row['error_detail']}"
        )
    finally:
        stub.stop()
    return user, project_id, state_id, pod_id, turn_id, stub.requests


def _backlog_state_id(db_conn, project_id):
    with db_conn.cursor() as cur:
        cur.execute(
            "SELECT id FROM states WHERE project_id = %s AND \"group\" = 'backlog'",
            (project_id,),
        )
        return cur.fetchone()[0]


def _tool_result(requests):
    """The tool call's result dict, from the follow-up request's tool message.

    Both lanes send the executed tool's return back to the model as an
    OpenAI ``role: tool`` message whose content is the result JSON, so the
    stub's captured payloads observe exactly what the tool returned.
    """
    assert len(requests) >= 2, "the turn makes a follow-up request after the tool call"
    messages = requests[1].get("messages") or []
    tools = [m for m in messages if isinstance(m, dict) and m.get("role") == "tool"]
    assert len(tools) == 1, "the follow-up carries exactly one tool result"
    return json.loads(tools[0]["content"])


def _assert_tool_created_issue(db_conn, project_id, state_id, tag):
    issues = _issue_rows(db_conn, project_id)
    assert len(issues) == 1, "the tool call created exactly one issue"
    issue = issues[0]
    assert issue["name"] == f"transition probe {tag}"
    assert str(issue["state_id"]) == str(state_id)
    assert issue["sequence_id"] == 1
    return issue


def _assert_ticker_armed(db_conn, issue, user_id):
    tickers = _ticker_rows(db_conn, issue["id"])
    assert len(tickers) == 1, "the entered-bucket fire arms exactly one ticker"
    ticker = tickers[0]
    assert ticker["enabled"] is True
    assert ticker["disarm_reason"] == ""
    assert ticker["next_run_at"] is not None
    assert ticker["used"] == 0
    assert ticker["granted"] == 0
    assert ticker["waited"] == 0
    assert ticker["pending_entry"] is False
    assert ticker["pending_entry_free"] is False
    assert ticker["pending_entry_actor_id"] is None
    assert ticker["pending_entry_trigger"] == ""
    assert ticker["resume_parent_run_id"] is None
    assert str(ticker["created_by_id"]) == str(user_id)
    return ticker


def test_create_issue_arms_ticker_without_pod(db_conn):
    """No pod: the fire arms the clock, no entry run (``no-pod-available``)."""
    user, project_id, state_id, _pod_id, _turn_id, _requests = _drive_create_issue(
        db_conn, "nopod"
    )
    issue = _assert_tool_created_issue(db_conn, project_id, state_id, "nopod")
    _assert_ticker_armed(db_conn, issue, user["id"])
    assert _run_rows(db_conn, issue["id"]) == []
    assert _comment_rows(db_conn, issue["id"]) == []


def test_create_issue_bounces_without_runner(db_conn):
    """Pod without a runner: the bounce posts, the ticker disarms, no run."""
    user, project_id, state_id, _pod_id, _turn_id, _requests = _drive_create_issue(
        db_conn, "bounce", pod=True
    )
    issue = _assert_tool_created_issue(db_conn, project_id, state_id, "bounce")
    assert str(issue["state_id"]) == str(state_id), "the issue stays in place"
    tickers = _ticker_rows(db_conn, issue["id"])
    assert len(tickers) == 1
    ticker = tickers[0]
    assert ticker["enabled"] is False
    assert ticker["disarm_reason"] == "left_ticking_state"
    assert ticker["next_run_at"] is None
    assert str(ticker["created_by_id"]) == str(user["id"])
    comments = _comment_rows(db_conn, issue["id"])
    assert len(comments) == 1, "the bounce posts exactly one comment"
    assert comments[0]["comment_stripped"] == BOUNCE_STRIPPED
    assert _run_rows(db_conn, issue["id"]) == []


def test_create_issue_dispatches_entry_run(db_conn):
    """Pod with a user-owned runner: an entry run dispatches immediately."""
    user, project_id, state_id, pod_id, _turn_id, _requests = _drive_create_issue(
        db_conn, "dispatch", pod=True, runner=True
    )
    issue = _assert_tool_created_issue(db_conn, project_id, state_id, "dispatch")
    _assert_ticker_armed(db_conn, issue, user["id"])
    runs = _run_rows(db_conn, issue["id"])
    assert len(runs) == 1, "the immediate dispatch creates exactly one entry run"
    run = runs[0]
    assert run["status"] == "queued"
    assert run["trigger"] == "state_transition"
    assert run["phase_kind"] == "coding-task"
    assert run["executor_kind"] == "local_runner"
    assert str(run["created_by_id"]) == str(user["id"])
    assert str(run["pod_id"]) == str(pod_id)
    assert str(run["work_item_id"]) == str(issue["id"])
    assert run["parent_run_id"] is None
    assert run["runner_id"] is None
    assert run["pinned_runner_id"] is None
    assert run["scheduler_binding_id"] is None
    assert run["error"] == ""
    assert run["error_code"] == ""
    assert run["dispatch_attempts"] == 0
    assert run["run_config"] == {"repo_ref": None, "repo_url": None, "git_work_branch": None}
    assert run["required_capabilities"] == []
    assert run["tool_plan"] == {}
    assert run["agent_metadata"] == {}
    assert "transition probe dispatch" in run["prompt"]
    assert _run_event_count(db_conn, run["id"]) == 0
    assert _comment_rows(db_conn, issue["id"]) == []


def test_create_issue_bounce_reports_backlog_tool_result(db_conn):
    """Bounce to a Backlog target: the DB row AND the tool result say Backlog.

    PIDASHCONV-1444, the Rust mirror of the Django oracle case: the
    drive's bounce moves the row to Backlog, and the tool result must
    report the post-drive row — the requested ticking state is the
    pre-fix failure.
    """
    user, project_id, _state_id, _pod_id, _turn_id, requests = _drive_create_issue(
        db_conn, "bouncereport", pod=True, backlog=True
    )
    backlog_id = _backlog_state_id(db_conn, project_id)
    issues = _issue_rows(db_conn, project_id)
    assert len(issues) == 1, "the tool call created exactly one issue"
    issue = issues[0]
    assert issue["name"] == "transition probe bouncereport"
    assert str(issue["state_id"]) == str(backlog_id), "the bounce moves the row to Backlog"
    assert issue["sequence_id"] == 1
    result = _tool_result(requests)
    assert result["created"] is True
    assert str(result["id"]) == str(issue["id"])
    assert result["state"] == "Backlog"
    assert result["state_group"] == "backlog"
    tickers = _ticker_rows(db_conn, issue["id"])
    assert len(tickers) == 1
    ticker = tickers[0]
    assert ticker["enabled"] is False
    assert ticker["disarm_reason"] == "left_ticking_state"
    assert ticker["next_run_at"] is None
    assert str(ticker["created_by_id"]) == str(user["id"])
    comments = _comment_rows(db_conn, issue["id"])
    assert len(comments) == 1, "the bounce posts exactly one comment"
    assert comments[0]["comment_stripped"] == BOUNCE_STRIPPED
    assert _run_rows(db_conn, issue["id"]) == []