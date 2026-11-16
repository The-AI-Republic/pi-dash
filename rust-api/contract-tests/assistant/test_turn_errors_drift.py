# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Status-first turn-error classification (PIDASHCONV-1566).

Port of the turn-runner half of Django main ``52df56b9e`` (PDASHOSS01-287,
PR #1430): ``_classify_error`` keeps the typed-error walk first, then asks
the ee seam (``turn_error_for_user``), which words the failure from the
provider's HTTP status — text markers are only a fallback for failures
that carry no status at all.

Stage-8 drift file: the asserted behavior exists only on origin/main (the
worker-plane CI serves Django from the checkout, frozen pre-#1430), so
this file is held out of the worker-plane pytest selection in
``rust-api-contract-tests.yml`` until the next main-sync merges main into
rust-dev (the PIDASHCONV-1063 holding pattern). Local proof: this file vs
main-Django plus the ``tasks_assistant/test_rust_replay.py`` mirrors vs
the PR Rust worker (see the issue PR). At the sync: add this file to the
worker-plane selection next to ``assistant/test_turn_errors.py``.

Cases (each: seed, publish ``assistant.run_turn`` in Celery wire format,
wait for ``failed``, diff the turn row / error row / ``turn_failed``
event):

- HTTP 429 with a request-quota body → ``provider_rate_limited`` (new
  code; Rust previously read ``internal``).
- HTTP 402 with an error body → ``provider_out_of_credit`` with
  ``" The provider said: <body message>"`` appended (Rust previously
  kept the bare detail).
- HTTP 400 ``unsupported model: ...`` → ``model_invalid`` naming the
  configured model (Rust previously read ``internal`` — ``unsupported
  model`` was not a marker — and used the old constant detail).
- HTTP 500 whose message mentions ``authentication`` → ``internal`` (a
  5xx never counts as auth; Rust previously read
  ``provider_auth_failed``); the same 500 mentioning ``connection`` →
  ``provider_unreachable`` on both.
- A 200 stream whose SSE error event carries a request id containing
  ``401`` (status-less text failure) → ``internal`` (the bare ``"401"``
  text marker is gone; Rust previously read ``provider_auth_failed``).
- The mid-stream ``model_not_found`` failure (moved from
  ``test_turn_errors.py::test_untyped_midstream_failure_keeps_text_classification``,
  whose detail changed): ``model_invalid`` naming the configured model.

The Rust worker mirrors live in ``tasks_assistant/test_rust_replay.py``.
"""

from __future__ import annotations

import json
import os
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from psycopg.rows import dict_row

from _harness import broker_probe, celery_wire
from _harness.auth import api_client, login_session_cookie
from _harness.db import wait_for_condition as wait_for
from _harness import seed as seed_helpers

RUN_TURN = "assistant.run_turn"

PASSWORD = "contract-suite-password"
MODEL_NAME = "oracle-stub"

RATE_LIMIT_DETAIL = "Too many requests to the model provider. Wait a moment and try again."
CREDIT_BODY = "Insufficient balance, top up at https://billing.example/topup"
CREDIT_DETAIL = f"The provider rejected the request for lack of credit. The provider said: {CREDIT_BODY}"
MODEL_DETAIL = f'The model "{MODEL_NAME}" was not accepted by the provider. Check the model name in Settings.'
UNREACHABLE_DETAIL = "Could not reach the configured provider endpoint."
INTERNAL_DETAIL = "The assistant hit an unexpected error."


# --------------------------------------------------------------------------- #
# Live oracle helpers (mirrors test_turn_errors.py; modules stay independent)
# --------------------------------------------------------------------------- #


def _base_url() -> str:
    return os.environ["BASE_URL"]


def _now_iso() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def _seed_member(db_conn, tag: str):
    seed_helpers.ensure_setup_done(db_conn)
    user = seed_helpers.user(db_conn, f"turnerr-{tag}")
    workspace = seed_helpers.workspace(db_conn, f"turnerr-{tag}", user["id"])
    seed_helpers.workspace_member(db_conn, workspace["id"], user["id"], role=15)
    cookie = login_session_cookie(_base_url(), user["email"], PASSWORD)
    return user, workspace, api_client(_base_url(), cookie)


def _seed_turn_trio(db_conn, user, workspace):
    thread_id, turn_id, message_id = (str(uuid.uuid4()) for _ in range(3))
    now = _now_iso()
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO assistant_thread "
            "(id, workspace_id, user_id, title, kind, is_archived, "
            "created_at, updated_at) "
            "VALUES (%s, %s, %s, %s, 'chat', FALSE, %s, %s)",
            (thread_id, str(workspace["id"]), str(user["id"]), "turn-errors drift", now, now),
        )
        cur.execute(
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


def _configure_llm(client, base_url):
    res = client.put(
        "/api/users/me/ai-assistant/config/",
        json={
            "provider_kind": "openai_compatible",
            "base_url": base_url,
            "model_name": MODEL_NAME,
            "api_key": "test-key-123",
        },
    )
    assert res.status_code == 200, res.text


class _FixedStatusStub:
    """Minimal OpenAI-compatible stub answering a fixed error status.

    Every ``POST`` (streaming or not) gets ``(status, body)``: the
    client's retry loop (429/5xx) runs against it, then the run fails
    with the surfaced HTTP error — status, unwrapped message and
    structured body all visible to the classifier, as with a real
    provider refusal.
    """

    def __init__(self, status: int, body: dict):
        self.status = status
        self.body = body
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
                raw = self.rfile.read(length) if length else b""
                stub.requests.append({"path": self.path, "bytes": len(raw)})
                content = json.dumps(stub.body).encode()
                self.send_response(stub.status)
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


class _StreamThenErrorStub:
    """Streams one chunk, then sends an SSE error event mid-stream.

    The client raises the embedded message out of the stream iteration
    with no HTTP status — the status-less text-failure shape. ``chunk``
    is the text sent before the error; ``error_message`` its message.
    """

    def __init__(self, chunk="Hello", error_message="boom"):
        self.chunk = chunk
        self.error_message = error_message
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
                if self.path.endswith("/chat/completions") and payload.get("stream"):
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream")
                    self.end_headers()
                    self.wfile.write(
                        (
                            'data: {"id":"stub","object":"chat.completion.chunk",'
                            '"created":0,"model":"oracle-stub","choices":[{"index":0,'
                            '"delta":{"content":'
                            + json.dumps(stub.chunk)
                            + '},"finish_reason":null}]}\n\n'
                        ).encode()
                    )
                    self.wfile.flush()
                    time.sleep(0.2)
                    self.wfile.write(
                        (
                            "data: "
                            + json.dumps({"error": {"message": stub.error_message}})
                            + "\n\n"
                        ).encode()
                    )
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


def _run_turn(db_conn, broker_url, tag: str, stub):
    """Seed, point the BYOK config at the stub, publish, wait for failed."""
    base = stub.start()
    try:
        user, workspace, client = _seed_member(db_conn, tag)
        _configure_llm(client, base)
        thread_id, turn_id, _msg = _seed_turn_trio(db_conn, user, workspace)

        celery_wire.publish(RUN_TURN, args=[turn_id], broker_url=broker_url)
        row = wait_for(
            lambda: _turn_row(db_conn, turn_id)
            if _turn_row(db_conn, turn_id)["status"] == "failed"
            else None,
            what=f"{tag} turn failed",
        )
        # `_fail_turn` commits the turn row first, then writes the error
        # row, the `turn_failed` event and the delta prune, so the failed
        # status alone does not mean those exist yet.
        def _failure_writes_complete():
            kinds = [e["kind"] for e in _turn_events(db_conn, turn_id)]
            if "turn_failed" in kinds and "assistant_delta" not in kinds:
                return kinds
            return None

        kinds = wait_for(_failure_writes_complete, what=f"{tag} failure writes")
        return thread_id, turn_id, row, kinds
    finally:
        stub.stop()


def _assert_failed_shape(db_conn, thread_id, turn_id, row, kinds, code, detail):
    assert row["error_code"] == code
    assert row["error_detail"] == detail
    assert row["completed_at"] is not None
    assert row["started_at"] is not None

    assert _thread_row(db_conn, thread_id)["active_turn_id"] is None

    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT display_content, status FROM assistant_message "
            "WHERE turn_id = %s AND kind = 'error'",
            (turn_id,),
        )
        err = cur.fetchone()
    assert err is not None and err["status"] == "failed"
    assert err["display_content"] == detail

    failed = next(e for e in _turn_events(db_conn, turn_id) if e["kind"] == "turn_failed")
    assert failed["payload"] == {
        "turn_id": turn_id,
        "error_code": code,
        "detail": detail,
    }
    assert "assistant_delta" not in kinds, "deltas are pruned on failure"


# --------------------------------------------------------------------------- #
# Live oracle: publish in Celery wire format, diff DB, capture SSE effects
# --------------------------------------------------------------------------- #


def test_worker_registration(broker_url):
    broker_probe.wait_for_registration({RUN_TURN})


def test_429_is_rate_limited(db_conn, broker_url):
    """A request-quota 429 asks the user to retry (new code)."""
    stub = _FixedStatusStub(
        429, {"error": {"message": "Rate limit reached for requests", "type": "requests"}}
    )
    thread_id, turn_id, row, kinds = _run_turn(db_conn, broker_url, "drift429", stub)
    assert len(stub.requests) == 3, "initial attempt plus two stainless retries"
    _assert_failed_shape(
        db_conn, thread_id, turn_id, row, kinds, "provider_rate_limited", RATE_LIMIT_DETAIL
    )


def test_402_keeps_the_providers_text(db_conn, broker_url):
    """A 402 appends the provider body's message to the credit detail."""
    stub = _FixedStatusStub(402, {"error": {"message": CREDIT_BODY}})
    thread_id, turn_id, row, kinds = _run_turn(db_conn, broker_url, "drift402", stub)
    assert len(stub.requests) == 1, "no retry on 402"
    _assert_failed_shape(
        db_conn, thread_id, turn_id, row, kinds, "provider_out_of_credit", CREDIT_DETAIL
    )


def test_400_unsupported_model_names_the_configured_model(db_conn, broker_url):
    """An unsupported-model 400 names the configured model and the setting."""
    stub = _FixedStatusStub(400, {"error": {"message": "unsupported model: foo"}})
    thread_id, turn_id, row, kinds = _run_turn(db_conn, broker_url, "drift400", stub)
    _assert_failed_shape(db_conn, thread_id, turn_id, row, kinds, "model_invalid", MODEL_DETAIL)


def test_500_mentioning_authentication_is_internal(db_conn, broker_url):
    """Auth text on a 5xx is not a key problem (status decides, not text)."""
    stub = _FixedStatusStub(500, {"error": {"message": "authentication service exploded"}})
    thread_id, turn_id, row, kinds = _run_turn(db_conn, broker_url, "drift500a", stub)
    assert len(stub.requests) == 3, "initial attempt plus two stainless retries"
    _assert_failed_shape(db_conn, thread_id, turn_id, row, kinds, "internal", INTERNAL_DETAIL)


def test_500_mentioning_connection_is_unreachable(db_conn, broker_url):
    """Unreachable text on a 5xx still reads as unreachable (both tables)."""
    stub = _FixedStatusStub(500, {"error": {"message": "connection reset by upstream"}})
    thread_id, turn_id, row, kinds = _run_turn(db_conn, broker_url, "drift500c", stub)
    _assert_failed_shape(
        db_conn, thread_id, turn_id, row, kinds, "provider_unreachable", UNREACHABLE_DETAIL
    )


def test_statusless_401_in_text_is_internal(db_conn, broker_url):
    """A request id containing 401 is not an API-key rejection."""
    stub = _StreamThenErrorStub(error_message="request req_401x7 failed: boom")
    thread_id, turn_id, row, kinds = _run_turn(db_conn, broker_url, "drift401t", stub)
    _assert_failed_shape(db_conn, thread_id, turn_id, row, kinds, "internal", INTERNAL_DETAIL)

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


def test_untyped_midstream_failure_names_the_configured_model(db_conn, broker_url):
    """One chunk streams, then an SSE error event arrives (moved from
    ``test_turn_errors.py``; the detail changed in #1430): the turn fails
    ``model_invalid`` naming the configured model, with the exact
    failed-turn shape — partial row failed, error row carries the detail,
    ``turn_failed`` payload, ``active_turn`` cleared."""
    stub = _StreamThenErrorStub(
        error_message="model_not_found: the model 'oracle-stub' does not exist"
    )
    thread_id, turn_id, row, kinds = _run_turn(db_conn, broker_url, "driftmid", stub)
    _assert_failed_shape(db_conn, thread_id, turn_id, row, kinds, "model_invalid", MODEL_DETAIL)

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
