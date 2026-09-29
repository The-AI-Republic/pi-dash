# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-06 task oracle: assistant turn pipeline + sweep (PIDASHCONV-254, F-A6-10).

Black-box contract for ``apps/api/pi_dash/assistant/tasks.py`` (526 lines):
``assistant.run_turn`` (load → claim → model → toolsets → stream → terminal
write) and ``assistant.sweep_stale_turns`` (fail RUNNING rows older than
330+60s). Task-only domain: jobs are published in Celery wire format, the DB
is diffed before/after, and the SSE side effects are captured from the
``assistant_event`` replay log (``events.py``: the SSE stream replays from
that table, so its rows *are* the SSE contract).

Static parity needs no backend (wire shape, decorator options, beat entry,
cancel-key shape, fixture vectors). Live tests need the task-oracle stack:
Django + a Celery worker on the broker ``CELERY_BROKER_URL`` names, Postgres
at ``DATABASE_URL``, Redis at ``CONTRACT_REDIS_URL`` (cancel keys), and the
server at ``BASE_URL``. Auth rides the public sign-in form (secret-free), and
turns are seeded straight into Postgres replicating the message-POST 202 row
shape (``views/messages.py``), so no stray task is ever enqueued: every live
test publishes its own wire message.

The complete/cancel paths need a provider that streams on demand. There is
no live third party here; ``_StubProvider`` below speaks just enough of the
OpenAI chat-completions protocol for the BYOK openai_compatible branch
(``runtime/llm.py:build_model``) to stream text deltas.
"""

from __future__ import annotations

import ast
import json
import os
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest
from psycopg.rows import dict_row

from _harness import broker_probe, celery_wire, taskspec
from _harness import redis as redis_helper
from _harness import seed as seed_helpers
from _harness.auth import api_client, login_session_cookie
from _harness.db import wait_for_condition as wait_for

RUN_TURN = "assistant.run_turn"
SWEEP = "assistant.sweep_stale_turns"
TASKS_MODULE = "pi_dash/assistant/tasks.py"

PASSWORD = "contract-suite-password"


# --------------------------------------------------------------------------- #
# Static parity: no infrastructure
# --------------------------------------------------------------------------- #


@pytest.mark.parametrize("task_name", [RUN_TURN, SWEEP])
def test_wire_payload_parity(task_name):
    """Payload parity without infra: the exact wire message for each task."""
    args = [str(uuid.uuid4())] if task_name == RUN_TURN else []
    message = celery_wire.capture_wire_message(task_name, args=args)
    celery_wire.assert_wire_message(message, task_name, args=args)


def test_run_turn_options_parity():
    """Ack/retry parity: no redelivery, 300/330 limits (``tasks.py:476-482``)."""
    taskspec.assert_task_options(
        TASKS_MODULE,
        "run_assistant_turn",
        {
            "name": "'assistant.run_turn'",
            "acks_late": "False",
            "max_retries": "0",
            "soft_time_limit": "TURN_SOFT_LIMIT",
            "time_limit": "TURN_HARD_LIMIT",
        },
    )
    # The limits behind the names: getattr(settings, …, default).
    source = taskspec.source_root() / TASKS_MODULE
    module = ast.parse(source.read_text())
    defaults = {}
    for node in module.body:
        if isinstance(node, ast.Assign) and len(node.targets) == 1:
            target = node.targets[0]
            if isinstance(target, ast.Name) and target.id in (
                "TURN_SOFT_LIMIT",
                "TURN_HARD_LIMIT",
            ):
                call = node.value
                assert isinstance(call, ast.Call), f"{target.id} must read settings"
                defaults[target.id] = ast.literal_eval(call.args[-1])
    assert defaults == {"TURN_SOFT_LIMIT": 300, "TURN_HARD_LIMIT": 330}


def test_sweep_options_parity():
    """The sweep keeps Celery defaults: named, no ack/retry overrides."""
    taskspec.assert_task_options(TASKS_MODULE, "sweep_stale_turns", {"name": "'assistant.sweep_stale_turns'"})
    for keyword in ("acks_late", "max_retries", "soft_time_limit", "time_limit"):
        taskspec.assert_no_option(TASKS_MODULE, "sweep_stale_turns", keyword)


def test_beat_entry_parity():
    taskspec.assert_beat_entry(
        "assistant-sweep-stale-turns",
        SWEEP,
        "timedelta(seconds=30)",
    )


def test_cancel_key_shape():
    """``cancel_key`` renders ``assistant:cancel:<turn_id>`` (``tasks.py:54-55``)."""
    source = taskspec.source_root() / TASKS_MODULE
    module = ast.parse(source.read_text())
    for node in module.body:
        if isinstance(node, ast.FunctionDef) and node.name == "cancel_key":
            ret = node.body[-1]
            assert isinstance(ret, ast.Return) and isinstance(ret.value, ast.JoinedStr)
            literal = "".join(
                v.value for v in ret.value.values if isinstance(v, ast.Constant)
            )
            assert literal == "assistant:cancel:", f"cancel prefix drifted: {literal!r}"
            return
    raise AssertionError("cancel_key not found")


def test_fixture_task_vectors_present():
    """F-A6-10 records the classifier vectors, cancel key and limits."""
    path = (
        taskspec.source_root().parent.parent
        / "rust-api"
        / "fixtures"
        / "assistant"
        / "tools-tasks.json"
    )
    assert path.exists(), f"fixture missing: {path}"
    tasks = json.loads(path.read_text())["tasks"]
    vectors = tasks["classify_error"]["vectors"]
    assert set(vectors) == {
        "payment_required",
        "status_402",
        "auth_401",
        "connection",
        "model_not_found",
        "other",
    }
    assert tasks["cancel_key"]["vector"] == "assistant:cancel:abc"
    assert tasks["turn_limits"] == {"soft": 300, "hard": 330, "delta_flush_ms": 100}


# --------------------------------------------------------------------------- #
# Live oracle helpers
# --------------------------------------------------------------------------- #


def _base_url() -> str:
    return os.environ["BASE_URL"]


def _now_iso() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def _seed_member(db_conn, tag: str):
    """One user + workspace + membership; returns (user, workspace, client)."""
    seed_helpers.ensure_setup_done(db_conn)
    user = seed_helpers.user(db_conn, f"turn-{tag}")
    workspace = seed_helpers.workspace(db_conn, f"turn-{tag}", user["id"])
    seed_helpers.workspace_member(db_conn, workspace["id"], user["id"], role=15)
    cookie = login_session_cookie(_base_url(), user["email"], PASSWORD)
    return user, workspace, api_client(_base_url(), cookie)


def _seed_turn_trio(db_conn, user, workspace, *, status="queued", started_at=None):
    """Replicate the message-POST 202 rows (``views/messages.py``).

    Thread (with ``active_turn`` set) + user message (seq 1, completed) +
    turn pointing at the message. Returns (thread_id, turn_id, message_id).
    """
    thread_id, turn_id, message_id = (str(uuid.uuid4()) for _ in range(3))
    now = _now_iso()
    # FK order (mirroring the 202 path's create order): thread first with no
    # active turn, then the turn, then the user message, then the backlinks.
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO assistant_thread "
            "(id, workspace_id, user_id, title, kind, is_archived, "
            "created_at, updated_at) "
            "VALUES (%s, %s, %s, %s, 'chat', FALSE, %s, %s)",
            (thread_id, str(workspace["id"]), str(user["id"]), "turn oracle", now, now),
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


def _wait_for_drain(broker_url: str, what: str):
    """Queue-drain wait on either broker transport.

    ``broker_probe.wait_for_queue_drain`` passive-declares the queue, which
    404s on the Redis broker once the list is fully consumed (an empty Redis
    list *is* drained). Poll ``LLEN`` directly for Redis; keep the probe for
    AMQP, where an empty queue still declares.
    """
    if broker_url.startswith("redis://"):
        import redis

        rdb = redis.Redis.from_url(broker_url, decode_responses=True)
        wait_for(lambda: (rdb.llen("celery") == 0) or None, what=what)
        rdb.close()
    else:
        broker_probe.wait_for_queue_drain(what=what)


def _configure_llm(client, base_url):
    res = client.put(
        "/api/users/me/ai-assistant/config/",
        json={
            "provider_kind": "openai_compatible",
            "base_url": base_url,
            "model_name": "oracle-stub",
            "api_key": "test-key-123",
        },
    )
    assert res.status_code == 200, res.text


# --------------------------------------------------------------------------- #
# Provider stub: just enough OpenAI chat-completions for a streaming turn
# --------------------------------------------------------------------------- #


class _StubProvider:
    """Minimal OpenAI-compatible stub for the BYOK openai_compatible branch.

    Serves ``POST /v1/chat/completions``: a non-streaming completion, or an
    SSE stream of one chunk per ``interval`` seconds (`` ["Hello", " world"]``
    by default) ending with ``data: [DONE]``. The slow stream lets the cancel
    test set the cancel key mid-turn: the worker only observes cancellation
    when the next event arrives, so chunks must keep coming.
    """

    def __init__(self, chunks=("Hello", " world"), interval=1.0):
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
                stub.requests.append({"path": self.path, "stream": payload.get("stream")})
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
# Live oracle: publish in Celery wire format, diff DB, capture SSE effects
# --------------------------------------------------------------------------- #


def test_worker_registration(broker_url):
    broker_probe.wait_for_registration({RUN_TURN, SWEEP})


def test_unknown_turn_is_noop(db_conn, broker_url):
    """A turn id with no eligible context changes nothing (``_load_context``)."""
    before = {
        table: _table_count(db_conn, table)
        for table in ("assistant_turn", "assistant_message", "assistant_event")
    }
    celery_wire.publish(RUN_TURN, args=[str(uuid.uuid4())], broker_url=broker_url)
    _wait_for_drain(broker_url, what="unknown turn consumed")
    after = {
        table: _table_count(db_conn, table)
        for table in ("assistant_turn", "assistant_message", "assistant_event")
    }
    assert before == after


def test_missing_config_fails_turn(db_conn, broker_url):
    """No BYOK config: model resolution fails the turn (``llm_config_missing``)."""
    user, workspace, _client = _seed_member(db_conn, "noconfig")
    thread_id, turn_id, _msg = _seed_turn_trio(db_conn, user, workspace)

    celery_wire.publish(RUN_TURN, args=[turn_id], broker_url=broker_url)
    row = wait_for(
        lambda: _turn_row(db_conn, turn_id) if _turn_row(db_conn, turn_id)["status"] == "failed" else None,
        what="missing-config turn failed",
    )
    assert row["error_code"] == "llm_config_missing"
    assert row["error_detail"] == "No LLM provider is configured for this user."
    assert row["completed_at"] is not None
    assert row["started_at"] is not None, "the claim stamps started_at first"
    # The thread is unwedged: the in-flight pointer is cleared.
    assert _thread_row(db_conn, thread_id)["active_turn_id"] is None

    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT kind, status, display_content FROM assistant_message "
            "WHERE turn_id = %s AND kind = 'error'",
            (turn_id,),
        )
        err = cur.fetchone()
    assert err is not None and err["status"] == "failed"
    assert err["display_content"] == row["error_detail"], "error row carries detail or code"

    kinds = [e["kind"] for e in _turn_events(db_conn, turn_id)]
    assert "turn_started" in kinds, f"claim event missing: {kinds}"
    assert "turn_failed" in kinds, f"failure event missing: {kinds}"
    failed = next(e for e in _turn_events(db_conn, turn_id) if e["kind"] == "turn_failed")
    assert failed["payload"]["error_code"] == "llm_config_missing"
    assert "assistant_delta" not in kinds, "deltas are pruned on failure"


def test_unreachable_provider_fails_turn(db_conn, broker_url):
    """Configured but dead endpoint: the full pipeline runs, then
    ``provider_unreachable`` (``_classify_error`` connection arm)."""
    user, workspace, client = _seed_member(db_conn, "unreach")
    _configure_llm(client, "http://127.0.0.1:9/v1")
    thread_id, turn_id, _msg = _seed_turn_trio(db_conn, user, workspace)

    celery_wire.publish(RUN_TURN, args=[turn_id], broker_url=broker_url)
    row = wait_for(
        lambda: _turn_row(db_conn, turn_id) if _turn_row(db_conn, turn_id)["status"] == "failed" else None,
        what="unreachable-provider turn failed",
    )
    assert row["error_code"] == "provider_unreachable"
    assert row["error_detail"] == "Could not reach the configured provider endpoint."
    kinds = [e["kind"] for e in _turn_events(db_conn, turn_id)]
    assert "turn_started" in kinds and "turn_failed" in kinds
    assert "assistant_delta" not in kinds


def test_sweep_fails_stale_turn(db_conn, broker_url):
    """A RUNNING turn older than 330+60s fails with ``turn_timeout``."""
    user, workspace, _client = _seed_member(db_conn, "stale")
    thread_id, turn_id, _msg = _seed_turn_trio(
        db_conn, user, workspace, status="running", started_at="2000-01-01T00:00:00Z"
    )

    celery_wire.publish(SWEEP, broker_url=broker_url)
    row = wait_for(
        lambda: _turn_row(db_conn, turn_id) if _turn_row(db_conn, turn_id)["status"] == "failed" else None,
        what="stale turn swept",
    )
    assert row["error_code"] == "turn_timeout"
    assert row["error_detail"] == "The assistant turn did not finish (worker lost)."
    assert _thread_row(db_conn, thread_id)["active_turn_id"] is None
    kinds = [e["kind"] for e in _turn_events(db_conn, turn_id)]
    assert "turn_failed" in kinds


def test_sweep_ignores_fresh_and_terminal(db_conn, broker_url):
    """Fresh RUNNING rows and terminal rows are untouched by the sweep."""
    user, workspace, _client = _seed_member(db_conn, "fresh")
    _fresh_thread, fresh_turn, _m1 = _seed_turn_trio(db_conn, user, workspace, status="running")
    _done_thread, done_turn, _m2 = _seed_turn_trio(db_conn, user, workspace, status="completed")

    celery_wire.publish(SWEEP, broker_url=broker_url)
    _wait_for_drain(broker_url, what="sweep consumed")
    # Settle: the sweep may still be running when the queue drains.
    time.sleep(2)
    assert _turn_row(db_conn, fresh_turn)["status"] == "running"
    assert _turn_row(db_conn, done_turn)["status"] == "completed"


def test_complete_turn_streams_and_prunes(db_conn, broker_url):
    """Stub provider: deltas stream, the turn completes, deltas are pruned."""
    stub = _StubProvider()
    base = stub.start()
    try:
        user, workspace, client = _seed_member(db_conn, "complete")
        _configure_llm(client, base)
        thread_id, turn_id, _msg = _seed_turn_trio(db_conn, user, workspace)

        celery_wire.publish(RUN_TURN, args=[turn_id], broker_url=broker_url)
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
        completed = next(e for e in _turn_events(db_conn, turn_id) if e["kind"] == "turn_completed")
        assert completed["payload"]["usage"] == row["usage"]
    finally:
        stub.stop()


def test_cancel_mid_stream_cancels_turn(db_conn, broker_url):
    """Cancel key set mid-stream: the turn cancels, the open row closes as
    cancelled, deltas are pruned (``views/cancel.py`` key contract)."""
    stub = _StubProvider(chunks=tuple(f"chunk-{i}" for i in range(30)), interval=1.0)
    base = stub.start()
    try:
        user, workspace, client = _seed_member(db_conn, "cancel")
        _configure_llm(client, base)
        _thread_id, turn_id, _msg = _seed_turn_trio(db_conn, user, workspace)

        celery_wire.publish(RUN_TURN, args=[turn_id], broker_url=broker_url)
        # Wait for a streamed delta (an assistant row is open and deltas are
        # flowing), then cancel through the real endpoint: same key, same TTL.
        # The worker observes the key when the next chunk arrives.
        wait_for(
            lambda: [e for e in _turn_events(db_conn, turn_id) if e["kind"] == "assistant_delta"]
            or None,
            what="first streamed delta",
        )
        res = client.post(f"/api/workspaces/{workspace['slug']}/ai-assistant/threads/{_thread_id}/cancel/")
        assert res.status_code == 204, res.text
        row = wait_for(
            lambda: _turn_row(db_conn, turn_id)
            if _turn_row(db_conn, turn_id)["status"] == "cancelled"
            else None,
            what="turn cancelled",
        )
        assert row["completed_at"] is not None
        assert row["error_code"] in (None, ""), "cancel writes no error code"

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

        # The cancel key the endpoint set follows the contract.
        rdb = redis_helper.client()
        assert rdb.get(f"assistant:cancel:{turn_id}") is not None
    finally:
        stub.stop()
