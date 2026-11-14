# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Assistant update_issue pod guard (PIDASHCONV-1735).

The assistant ``update_issue`` tool refuses a pod change while the issue
has an active run (``Issue.has_active_run`` — every run in
``NON_TERMINAL_STATUSES``, queued and paused included). Live oracle: seed a
project with two pods, an issue assigned to P1, and an ``agent_run`` for
the issue in the stated status, drive one turn whose model emits
``update_issue(issue_id, assigned_pod=<P2>)``, and prove the refusal (the
``role: tool`` payload carries the retry text, ``issues.assigned_pod_id``
stays P1) or the change (pod becomes P2).

Needs a worker on both backends (the turn executes there), so this file is
listed in ``TASK_ORACLE_IGNORES.txt`` like
``test_update_issue_completion.py``. Backend-agnostic: the turn is triggered
through the message POST and the assertions poll Postgres plus the stub's
captured tool payloads.
"""

from __future__ import annotations

import json
import os
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from psycopg.rows import dict_row

from _harness import seed as seed_helpers
from _harness.auth import api_client, login_session_cookie
from _harness.db import wait_for_condition as wait_for

PASSWORD = "contract-suite-password"

POD_GUARD = "cannot reassign pod while the issue has an active run"


def _base_url() -> str:
    return os.environ["BASE_URL"]


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


class _PodToolCallStub:
    """OpenAI-compatible stub whose first completion is one pod update.

    Serves ``POST /v1/chat/completions`` in streaming and non-streaming
    form. Until a ``role: tool`` message appears in the request history
    (both lanes send tool results and retry prompts back as OpenAI
    ``tool``-role messages), the stub answers with a single
    ``update_issue`` call; afterwards it streams a short final text so
    the turn completes.
    """

    def __init__(self, issue_id: str, pod_id: str):
        self.arguments = json.dumps(
            {"issue_id": issue_id, "assigned_pod": pod_id}
        )
        self.requests: list[dict] = []
        self.payloads: list[dict] = []
        self._server = None
        self._thread = None
        self._lock = threading.Lock()

    def start(self) -> str:
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
                messages = payload.get("messages") or []
                already_ran = any(
                    isinstance(m, dict) and m.get("role") == "tool" for m in messages
                )
                with stub._lock:
                    stub.requests.append(
                        {
                            "path": self.path,
                            "stream": payload.get("stream"),
                            "tool_result_seen": already_ran,
                        }
                    )
                    stub.payloads.append(payload)
                if self.path.endswith("/chat/completions") and payload.get("stream"):
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream")
                    self.end_headers()
                    if already_ran:
                        self.wfile.write(
                            (
                                'data: {"id":"stub","object":"chat.completion.chunk",'
                                '"created":0,"model":"oracle-stub","choices":[{"index":0,'
                                '"delta":{"role":"assistant","content":"Done"},'
                                '"finish_reason":null}]}\n\n'
                            ).encode()
                        )
                        self.wfile.write(
                            (
                                'data: {"id":"stub","object":"chat.completion.chunk",'
                                '"created":0,"model":"oracle-stub","choices":[{"index":0,'
                                '"delta":{},"finish_reason":"stop"}]}\n\n'
                            ).encode()
                        )
                    else:
                        self.wfile.write(
                            (
                                'data: {"id":"stub","object":"chat.completion.chunk",'
                                '"created":0,"model":"oracle-stub","choices":[{"index":0,'
                                '"delta":{"role":"assistant","content":null,'
                                '"tool_calls":[{"index":0,"id":"call_update_1",'
                                '"type":"function","function":{"name":"update_issue",'
                                '"arguments":'
                                + json.dumps(stub.arguments)
                                + '}}]},"finish_reason":null}]}\n\n'
                            ).encode()
                        )
                        self.wfile.write(
                            (
                                'data: {"id":"stub","object":"chat.completion.chunk",'
                                '"created":0,"model":"oracle-stub","choices":[{"index":0,'
                                '"delta":{},"finish_reason":"tool_calls"}]}\n\n'
                            ).encode()
                        )
                    self.wfile.write(b"data: [DONE]\n\n")
                    self.wfile.flush()
                else:
                    if already_ran:
                        message = {"role": "assistant", "content": "Done"}
                        finish = "stop"
                    else:
                        message = {
                            "role": "assistant",
                            "content": None,
                            "tool_calls": [
                                {
                                    "id": "call_update_1",
                                    "type": "function",
                                    "function": {
                                        "name": "update_issue",
                                        "arguments": stub.arguments,
                                    },
                                }
                            ],
                        }
                        finish = "tool_calls"
                    content = json.dumps(
                        {
                            "id": "stub",
                            "object": "chat.completion",
                            "created": 0,
                            "model": "oracle-stub",
                            "choices": [
                                {
                                    "index": 0,
                                    "message": message,
                                    "finish_reason": finish,
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


def _turn_row(db_conn, turn_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT * FROM assistant_turn WHERE id = %s", (turn_id,))
        return cur.fetchone()


def _issue_pod(db_conn, issue_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT assigned_pod_id FROM issues WHERE id = %s", (issue_id,))
        return cur.fetchone()["assigned_pod_id"]


def _tool_contents(payloads):
    """Every ``role: tool`` content across the stub's captured payloads.

    Both lanes send tool returns (result JSON) and ``ModelRetry`` prompts
    (retry text) back to the model as OpenAI ``role: tool`` messages, so
    one string list covers the success and refusal shapes.
    """
    found = []
    for payload in payloads:
        for message in payload.get("messages") or []:
            if isinstance(message, dict) and message.get("role") == "tool":
                content = message.get("content")
                if isinstance(content, str):
                    found.append(content)
                else:
                    found.append(json.dumps(content))
    return found


def _seed_world(db_conn, tag: str, run_status: str | None):
    """Seed owner/workspace/project, two pods, and the issue on P1.

    Optionally seeds one ``agent_run`` for the issue on P1 with
    ``run_status``. Returns ``(owner, ws, project, issue_id, p1_id, p2_id)``.
    """
    seed_helpers.ensure_setup_done(db_conn)
    chain = seed_helpers.issue_chain(db_conn, tag)
    owner, ws, project = chain["owner"], chain["workspace"], chain["project"]
    seed_helpers.workspace_member(db_conn, str(ws["id"]), str(owner["id"]), role=15)
    seed_helpers.project_member(
        db_conn, str(project["id"]), str(ws["id"]), str(owner["id"]), role=15
    )
    seed = seed_helpers.Seed(db_conn)
    p1_id = str(seed.pod(str(ws["id"]), str(project["id"])))
    # Second pod must not be the project default (partial unique index).
    p2_id = str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO pod (id, name, description, is_default, workspace_id, project_id, created_at, updated_at) "
            "VALUES (%s, %s, '', FALSE, %s, %s, now(), now())",
            (p2_id, f"ct1735-pod-{uuid.uuid4().hex[:6]}", str(ws["id"]), str(project["id"])),
        )
    issue_id = str(chain["issue"]["id"])
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET assigned_pod_id = %s WHERE id = %s",
            (p1_id, issue_id),
        )
    if run_status is not None:
        run_id = str(uuid.uuid4())
        with db_conn.cursor() as cur:
            cur.execute(
                """INSERT INTO agent_run
                   (id, status, prompt, run_config, required_capabilities, thread_id,
                    error_details, workspace_id, work_item_id, pod_id, created_by_id, llm_model,
                    trigger, executor_kind, dispatch_attempts,
                    cancel_reason, tool_plan, phase_kind, agent_metadata,
                    usage, pin_kind, scheduler_binding_id, created_at)
                   VALUES (%s,%s,'contract prompt','{}','{}','contract-thread','{}',
                           %s,%s,%s,%s,'contract-model', 'human', 'local_runner', 0,
                           '', '{}', 'work', '{}', '{"input": 0, "output": 0, "total": 0}', 'locality',
                           NULL, now())""",
                (run_id, run_status, str(ws["id"]), issue_id, p1_id, str(owner["id"])),
            )
    db_conn.commit()
    return owner, ws, project, issue_id, p1_id, p2_id


def _drive_pod_change(db_conn, tag, run_status):
    """Seed, drive one turn emitting the pod change, return the pieces."""
    owner, ws, project, issue_id, p1_id, p2_id = _seed_world(db_conn, tag, run_status)
    stub = _PodToolCallStub(issue_id, p2_id)
    base = stub.start()
    try:
        cookie = login_session_cookie(_base_url(), owner["email"], PASSWORD)
        client = api_client(_base_url(), cookie)
        _configure_llm(client, base)
        threads_base = f"/api/workspaces/{ws['slug']}/ai-assistant"
        res = client.post(f"{threads_base}/threads/", json={"title": "move pod"})
        assert res.status_code == 201, res.text
        thread_id = res.json()["id"]
        res = client.post(
            f"{threads_base}/threads/{thread_id}/messages/",
            json={"content": "Please move the issue to the other pod."},
        )
        assert res.status_code == 202, res.text
        turn_id = res.json()["turn"]["id"]
        row = wait_for(
            lambda: _turn_row(db_conn, turn_id)
            if _turn_row(db_conn, turn_id)["status"] == "completed"
            else None,
            what="pod-change turn completed",
            timeout=180,
        )
        assert row["status"] == "completed"
        return issue_id, p1_id, p2_id, stub.payloads
    finally:
        stub.stop()


def test_pod_change_refused_with_queued_run(db_conn):
    """A queued run blocks the pod change (PIDASHCONV-1735).

    The ``agent_run`` status is ``queued`` on P1. Django's
    ``has_active_run`` counts it; the pre-fix Rust gate used the E8
    blocking set, which drops ``Queued``, so the turn used to change the
    pod. After the fix the tool retries with the guard text and the row
    stays on P1.
    """
    issue_id, p1_id, _p2_id, payloads = _drive_pod_change(
        db_conn, "1735queued", "queued"
    )
    assert str(_issue_pod(db_conn, issue_id)) == p1_id, (
        "queued run must keep the pod on P1"
    )
    contents = _tool_contents(payloads)
    assert contents, "the turn must send one tool result or retry prompt"
    assert any(POD_GUARD in c for c in contents), (
        f"retry text must carry the guard; got {contents!r}"
    )


def test_pod_change_refused_with_paused_run(db_conn):
    """A ``paused_awaiting_input`` run blocks the pod change (PIDASHCONV-1735)."""
    issue_id, p1_id, _p2_id, payloads = _drive_pod_change(
        db_conn, "1735paused", "paused_awaiting_input"
    )
    assert str(_issue_pod(db_conn, issue_id)) == p1_id, (
        "paused run must keep the pod on P1"
    )
    contents = _tool_contents(payloads)
    assert contents, "the turn must send one tool result or retry prompt"
    assert any(POD_GUARD in c for c in contents), (
        f"retry text must carry the guard; got {contents!r}"
    )


def test_pod_change_allowed_without_run(db_conn):
    """With no run the pod change goes through (PIDASHCONV-1735)."""
    issue_id, _p1_id, p2_id, payloads = _drive_pod_change(
        db_conn, "1735norun", None
    )
    assert str(_issue_pod(db_conn, issue_id)) == p2_id, (
        "no-run pod change must move the row to P2"
    )
    contents = _tool_contents(payloads)
    assert contents, "the turn must send one tool result"
    parsed = []
    for c in contents:
        try:
            parsed.append(json.loads(c))
        except ValueError:
            continue
    assert parsed and parsed[0].get("updated") is True, (
        f"success result must report updated; got {parsed!r}"
    )
    assert "pod" in (parsed[0].get("changed") or []), (
        f"success result must list pod in changed; got {parsed[0]!r}"
    )
