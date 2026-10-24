# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Typed turn-failure codes survive the run (PIDASHCONV-1062).

Port of Django's ``apps/api/pi_dash/tests/contract/assistant/test_turn_errors.py``
(new in ``bb3d99c6``, PR #1126): before text-guessing a code, ``_classify_error``
walks ``exc.__cause__`` and returns a typed ``AssistantError``'s own code and
detail.

What ports where:

- The classifier unit cases (typed direct, typed wrapped, untyped text) are
  pinned as fixture vectors (``tools-tasks.json`` ``typed_vectors``) and
  executed as Rust unit tests
  (``run_turn.rs::drive_typed_errors_keep_code_and_detail``); this file asserts
  the fixture presence and byte-exactness against Django's own test constants.
- The typed-first branch itself is asserted statically against Django source
  (AST): ``_classify_error`` returns an ``AssistantError``'s ``(code, detail)``
  before any text matching. Until PIDASHCONV-1055 merges ``main`` into
  ``rust-dev``, that source lives on ``origin/main`` only, so the test reads
  ``PIDASH_DJANGO_MAIN_DIR`` (default: the checked-out ``apps/api``) — the
  PIDASHCONV-1058 pattern. CI provisions a pinned ``origin/main`` worktree for
  the worker-plane job; retire the variable after the merge.
- The whole-runner case (text streams, *then* the run fails) ports as a live
  task-oracle test below. OSS has no black-box stimulus that raises a typed
  error mid-run — the typed producers are model adapters / ee providers in the
  private overlay (zero ``raise QuotaExceeded`` / ``raise Provider*`` in OSS
  ``apps/api``; Django's own test monkeypatches the provider) — so the live
  test drives the closest triggerable path: the stub streams one chunk and then
  sends an SSE error event, and the turn must fail with the *text*
  classification plus the exact failed-turn shape (partial row failed, error
  row carries the detail, ``turn_failed`` payload, ``active_turn`` cleared).
  (An abrupt TCP close mid-stream does NOT fail the run — the OpenAI client
  treats the truncated stream as complete — so the error event is the only
  black-box mid-stream failure; verified empirically.) The typed mid-run path
  on Django is covered by Django's own suite; the typed mid-run path on Rust by
  the unit test above.

A seam fault (PIDASHCONV-1254) ports as a second live case below: a
turn-scoped trigger fails the first ``assistant_delta`` INSERT — the
black-box shape of ``_emit_delta`` raising out of ``streamer.handle`` — and
the turn must fail at once (``internal`` + ``turn_failed``), not at the
390s sweep. The Rust replay mirrors it in
``tasks_assistant/test_rust_replay.py``.
"""

from __future__ import annotations

import ast
import json
import os
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from psycopg.rows import dict_row

from _harness import broker_probe, celery_wire, taskspec
from _harness.auth import api_client, login_session_cookie
from _harness.db import wait_for_condition as wait_for
from _harness import seed as seed_helpers

RUN_TURN = "assistant.run_turn"
TASKS_MODULE = "pi_dash/assistant/tasks.py"
ERRORS_MODULE = "pi_dash/assistant/errors.py"
DJANGO_TEST_MODULE = "pi_dash/tests/contract/assistant/test_turn_errors.py"

PASSWORD = "contract-suite-password"

# The origin/main commit the static tests below are pinned to (PIDASHCONV-1055
# audit tip; CI provisions exactly this worktree for the worker-plane job).
MAIN_PIN = "3267ceb9cda65c90e6cb66e4a3ef8aaa26ca3a4e"


def _django_main_root() -> Path:
    """Django source root carrying the #1126 change (usually a main worktree)."""
    override = os.environ.get("PIDASH_DJANGO_MAIN_DIR")
    if override:
        return Path(override)
    return taskspec.source_root()


# --------------------------------------------------------------------------- #
# Static parity: no infrastructure
# --------------------------------------------------------------------------- #


def _classify_fn():
    path = _django_main_root() / TASKS_MODULE
    assert path.exists(), f"Django source not found: {path}"
    module = ast.parse(path.read_text())
    for node in module.body:
        if isinstance(node, ast.FunctionDef) and node.name == "_classify_error":
            return node, path.read_text()
    raise AssertionError(f"_classify_error not found in {path}")


def test_classify_error_typed_branch_first():
    """``_classify_error`` returns an ``AssistantError``'s own code/detail from
    the cause chain before any text matching (``tasks.py:443-451``)."""
    fn, source = _classify_fn()

    # The cause-chain walk precedes the text fallback in source order.
    walk_at = source.index("cause.__cause__")
    text_at = source.index("text = str(exc).lower()")
    assert walk_at < text_at, "typed walk must precede text matching"

    # Structural shape: while over the chain, isinstance gate, (code, detail).
    loops = [n for n in ast.walk(fn) if isinstance(n, ast.While)]
    assert loops, "no cause-chain loop in _classify_error"
    for loop in loops:
        for node in ast.walk(loop):
            if not isinstance(node, ast.If):
                continue
            test = node.test
            if (
                isinstance(test, ast.Call)
                and getattr(test.func, "id", "") == "isinstance"
                and len(test.args) == 2
                and getattr(test.args[1], "id", "") == "AssistantError"
            ):
                rets = [n for n in ast.walk(node) if isinstance(n, ast.Return)]
                assert rets, "isinstance gate returns nothing"
                value = rets[0].value
                assert isinstance(value, ast.Tuple) and len(value.elts) == 2, (
                    "typed branch must return a (code, detail) pair"
                )
                attrs = [
                    getattr(e, "attr", "") for e in value.elts
                    if isinstance(e, ast.Attribute)
                ]
                assert attrs == ["code", "detail"], f"typed pair drifted: {attrs}"
                return
    raise AssertionError("no isinstance(cause, AssistantError) gate in _classify_error")


def test_typed_fixture_vectors_match_django():
    """The fixture pins Django's own test constants byte for byte."""
    root = _django_main_root()
    test_path = root / DJANGO_TEST_MODULE
    assert test_path.exists(), (
        f"Django test not found: {test_path} "
        "(needs PIDASH_DJANGO_MAIN_DIR at a main worktree until PIDASHCONV-1055 merges)"
    )
    test_src = test_path.read_text()
    test_mod = ast.parse(test_src)
    usage_detail = None
    for node in test_mod.body:
        if (
            isinstance(node, ast.Assign)
            and len(node.targets) == 1
            and getattr(node.targets[0], "id", "") == "USAGE_DETAIL"
        ):
            usage_detail = ast.literal_eval(node.value)
    assert usage_detail, "USAGE_DETAIL not found in Django's test_turn_errors.py"
    assert '"Stream ended early."' in test_src, "unreachable detail drifted in Django test"

    errors_path = root / ERRORS_MODULE
    errors_mod = ast.parse(errors_path.read_text())
    codes = {}
    for node in errors_mod.body:
        if isinstance(node, ast.ClassDef) and node.name in ("QuotaExceeded", "ProviderUnreachable"):
            for stmt in node.body:
                if (
                    isinstance(stmt, ast.Assign)
                    and len(stmt.targets) == 1
                    and getattr(stmt.targets[0], "id", "") == "code"
                ):
                    codes[node.name] = ast.literal_eval(stmt.value)
    assert codes == {"QuotaExceeded": "quota_exceeded", "ProviderUnreachable": "provider_unreachable"}, (
        f"error codes drifted: {codes}"
    )

    fx_path = (
        taskspec.source_root().parent.parent
        / "rust-api"
        / "fixtures"
        / "assistant"
        / "tools-tasks.json"
    )
    assert fx_path.exists(), f"fixture missing: {fx_path}"
    typed = json.loads(fx_path.read_text())["tasks"]["classify_error"]["typed_vectors"]
    assert set(typed["vectors"]) == {"quota_direct", "unreachable_direct"}
    assert typed["vectors"]["quota_direct"] == ["quota_exceeded", usage_detail]
    assert typed["vectors"]["unreachable_direct"] == ["provider_unreachable", "Stream ended early."]


def test_text_vectors_unchanged():
    """The #1126 port adds typed vectors; the six text vectors are untouched."""
    fx_path = (
        taskspec.source_root().parent.parent
        / "rust-api"
        / "fixtures"
        / "assistant"
        / "tools-tasks.json"
    )
    vectors = json.loads(fx_path.read_text())["tasks"]["classify_error"]["vectors"]
    assert set(vectors) == {
        "payment_required",
        "status_402",
        "auth_401",
        "connection",
        "model_not_found",
        "other",
    }


# --------------------------------------------------------------------------- #
# Live oracle helpers (mirrors test_tasks_turn.py; modules stay independent)
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
            (thread_id, str(workspace["id"]), str(user["id"]), "turn-errors oracle", now, now),
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
            "model_name": "oracle-stub",
            "api_key": "test-key-123",
        },
    )
    assert res.status_code == 200, res.text


# --------------------------------------------------------------------------- #
# Provider stub: streams one chunk, then sends an SSE error event mid-stream
# --------------------------------------------------------------------------- #

# Real provider error code, mid-stream: the OpenAI client raises APIError with
# this message, which carries no typed wrapper, so _classify_error must route
# it by text (model_invalid) — the untyped half of Django's stream_then_fail.
MIDSTREAM_ERROR_MESSAGE = "model_not_found: the model 'oracle-stub' does not exist"


class _StreamThenErrorStub:
    """Minimal OpenAI-compatible stub for the BYOK openai_compatible branch.

    Serves ``POST /v1/chat/completions``: on a streaming request it sends one
    SSE text chunk, flushes, then sends an SSE error event and closes cleanly.
    The client raises the error out of the stream iteration, so the run fails
    with an untyped provider error after text has streamed — the black-box
    shape of Django's ``stream_then_fail`` case. With
    ``error_message=None`` the stub ends the stream cleanly (``[DONE]``)
    instead, for tests whose failure comes from elsewhere (a seam fault).
    """

    def __init__(self, chunk="Hello", error_message=MIDSTREAM_ERROR_MESSAGE):
        self.chunk = chunk
        self.error_message = error_message
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
                    if stub.error_message is None:
                        # Clean end: the failure under test comes from
                        # elsewhere (a seam fault), not the provider.
                        self.wfile.write(b"data: [DONE]\n\n")
                        self.wfile.flush()
                    else:
                        # Mid-stream error event: the client raises APIError
                        # with the embedded message out of the stream
                        # iteration.
                        self.wfile.write(
                            (
                                "data: "
                                + json.dumps(
                                    {
                                        "error": {
                                            "message": stub.error_message,
                                            "type": "invalid_request_error",
                                            "code": "model_not_found",
                                        }
                                    }
                                )
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


# --------------------------------------------------------------------------- #
# Live oracle: publish in Celery wire format, diff DB, capture SSE effects
# --------------------------------------------------------------------------- #


def test_worker_registration(broker_url):
    broker_probe.wait_for_registration({RUN_TURN})


def test_untyped_midstream_failure_keeps_text_classification(db_conn, broker_url):
    """One chunk streams, then an SSE error event arrives: the turn fails with
    the text classification (``_classify_error`` fallback, unchanged by #1126)
    and the exact failed-turn shape — partial row failed, error row carries
    the detail, ``turn_failed`` payload, ``active_turn`` cleared."""
    stub = _StreamThenErrorStub()
    base = stub.start()
    try:
        user, workspace, client = _seed_member(db_conn, "midstream")
        _configure_llm(client, base)
        thread_id, turn_id, _msg = _seed_turn_trio(db_conn, user, workspace)

        celery_wire.publish(RUN_TURN, args=[turn_id], broker_url=broker_url)
        row = wait_for(
            lambda: _turn_row(db_conn, turn_id)
            if _turn_row(db_conn, turn_id)["status"] == "failed"
            else None,
            what="midstream-error turn failed",
        )
        # `_fail_turn` commits the turn row first, then writes the error row,
        # the `turn_failed` event and the delta prune (`tasks.py:181-201`),
        # so the failed status alone does not mean those exist yet. Wait for
        # the last of them before reading any (else the error-row read races).
        def _failure_writes_complete():
            kinds = [e["kind"] for e in _turn_events(db_conn, turn_id)]
            if "turn_failed" in kinds and "assistant_delta" not in kinds:
                return kinds
            return None

        kinds = wait_for(_failure_writes_complete, what="midstream-error failure writes")
        assert row["error_code"] == "model_invalid"
        assert row["error_detail"] == "The configured model name was not accepted by the provider."
        assert row["completed_at"] is not None
        assert row["started_at"] is not None

        assert _thread_row(db_conn, thread_id)["active_turn_id"] is None

        # The text that had already streamed is closed out as failed, and the
        # user-visible error row carries the actionable detail.
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
            "error_code": "model_invalid",
            "detail": "The configured model name was not accepted by the provider.",
        }
        assert "assistant_delta" not in kinds, "deltas are pruned on failure"
    finally:
        stub.stop()


# --------------------------------------------------------------------------- #
# Seam fault injection: a trigger fails one turn's delta writes (PIDASHCONV-1254)
# --------------------------------------------------------------------------- #

#: The trigger's RAISE text. It matches no ``_classify_error`` marker, so a
#: mid-stream seam fault classifies ``internal`` on both backends.
SEAM_FAULT_MESSAGE = "seam fault injection: assistant_delta blocked"

#: The sweep only fails turns older than 390s; failing inside the 60s task
#: wait proves the turn failed at once, not via the sweep.
SWEEP_CUTOFF_SECS = 390


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
    return fn, trg


def _remove_delta_fault(db_conn, fn: str, trg: str):
    with db_conn.cursor() as cur:
        cur.execute(f"DROP TRIGGER IF EXISTS {trg} ON assistant_event")
        cur.execute(f"DROP FUNCTION IF EXISTS {fn}()")


def test_midstream_seam_failure_fails_turn_at_once(db_conn, broker_url):
    """A DB error from a mid-stream row write fails the turn at once.

    The trigger fails the first ``assistant_delta`` INSERT — the black-box
    shape of ``_emit_delta`` raising out of ``streamer.handle``. Django
    catches it in the run's ``except Exception`` arm (``tasks.py:356-361``):
    the open row fails, the error classifies ``internal``, and the turn fails
    with ``turn_failed`` — immediately, not at the 390s sweep. The Rust replay
    in ``tasks_assistant/test_rust_replay.py`` pins the same shape on the
    Rust worker.
    """
    stub = _StreamThenErrorStub(error_message=None)
    base = stub.start()
    tag = uuid.uuid4().hex[:8]
    fn = trg = None
    try:
        user, workspace, client = _seed_member(db_conn, f"seam{tag}")
        _configure_llm(client, base)
        thread_id, turn_id, _msg = _seed_turn_trio(db_conn, user, workspace)
        fn, trg = _install_delta_fault(db_conn, tag, turn_id)

        t0 = time.monotonic()
        celery_wire.publish(RUN_TURN, args=[turn_id], broker_url=broker_url)
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
