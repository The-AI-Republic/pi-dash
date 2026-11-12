# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Assistant dispatch_coding_run with a failing entry prompt (PIDASHCONV-1578).

Stage-8 follow-up to PIDASHCONV-1571: the identical ``turn transition
finalized a run`` fail-closed in the third turn drive,
``drive_transition`` (serves ``dispatch_coding_run`` only since
PIDASHCONV-1124).

Live oracle: seed a project with a pod and an agent-eligible state plus
a user-owned runner, and a workspace ``prompt_section_override`` row
for the overridable ``default-posture`` coding-task section whose body
fails to parse on both engines (``{% if %}``). Run a turn whose model
emits a single ``dispatch_coding_run`` call into the ticking state.

The prompt-build failure path finalizes the entry run FAILED and
returns ``render-failed`` with no exception
(``orchestration/service.py:820-832``), so Django commits every write
and the tool returns its normal outcome. Rust must do the same: the
failed run row commits, the tool result is the normal dispatch
outcome, and the turn completes.

Needs a worker on both backends (the turn executes there), so this
file is listed in ``TASK_ORACLE_IGNORES.txt`` like
``test_update_issue_completion.py``. Backend-agnostic: the turn is
triggered through the message POST (both backends enqueue
``assistant.run_turn`` from it) and the assertions poll Postgres, with
the tool result observed through the stub's captured payloads.
"""

from __future__ import annotations

import json
import os
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from psycopg.rows import dict_row

from _harness import seed as seed_helpers
from _harness.auth import api_client, login_session_cookie
from _harness.db import wait_for_condition as wait_for

PASSWORD = "contract-suite-password"

#: Override body that fails to parse on both template engines (Jinja2 on
#: Django, minijinja strict on Rust), so the entry-run prompt build raises
#: inside the transition (PIDASHCONV-1571). ``default-posture`` is an
#: ``overridable`` coding-task section, so a workspace-level row wins
#: over the registry default on both backends.
BROKEN_OVERRIDE_SECTION = "default-posture"
BROKEN_OVERRIDE_BODY = "{% if %}\n"


def _base_url() -> str:
    return os.environ["BASE_URL"]


def _database_url() -> str:
    return os.environ["DATABASE_URL"]


class _ToolCallStub:
    """OpenAI-compatible stub whose first completion is one tool call.

    Serves ``POST /v1/chat/completions`` in streaming and non-streaming
    form. Until a ``role: tool`` message appears in the request history
    (both the Django pydantic-ai lane and the Rust turn lane send tool
    results back as OpenAI ``tool``-role messages), the stub answers
    with a single ``dispatch_coding_run`` call; afterwards it streams a
    short final text so the turn completes.
    """

    def __init__(self, issue_id: str, state_id: str):
        self.arguments = json.dumps(
            {"issue_id": issue_id, "target_state_id": state_id}
        )
        self.requests: list[dict] = []
        #: Full request payloads: the follow-up request's ``role: tool``
        #: message carries the tool-result JSON.
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
                                '"tool_calls":[{"index":0,"id":"call_dispatch_1",'
                                '"type":"function","function":{"name":"dispatch_coding_run",'
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
                                    "id": "call_dispatch_1",
                                    "type": "function",
                                    "function": {
                                        "name": "dispatch_coding_run",
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


def _turn_row(db_conn, turn_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT * FROM assistant_turn WHERE id = %s", (turn_id,))
        return cur.fetchone()


def _issue_state(db_conn, issue_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT state_id FROM issues WHERE id = %s", (issue_id,))
        return cur.fetchone()["state_id"]


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


def _tool_results(payloads):
    """Every tool-result dict across the stub's captured payloads.

    Both lanes send the executed tool's return back to the model as an
    OpenAI ``role: tool`` message whose content is the result JSON, so the
    stub observes exactly what the tool returned.
    """
    found = []
    for payload in payloads:
        for message in payload.get("messages") or []:
            if isinstance(message, dict) and message.get("role") == "tool":
                found.append(json.loads(message["content"]))
    return found


def test_dispatch_coding_run_prompt_failure_commits_failed_run(db_conn):
    """Prompt build failure on dispatch: the run commits, the tool briefs.

    PIDASHCONV-1578: a turn dispatches a coding run on a Todo issue into
    a ticking state with a pod and an eligible runner, but the entry-run
    prompt build raises (broken workspace override). The prompt path
    finalizes the run FAILED and returns ``render-failed`` with no
    exception (``orchestration/service.py:820-832``), so the tool returns
    its normal dispatch outcome — ``dispatched`` with the failed run's
    id — and the turn completes. Backend-agnostic like the
    PIDASHCONV-1571 update-path sibling.
    """
    seed_helpers.ensure_setup_done(db_conn)
    chain = seed_helpers.issue_chain(db_conn, "1578dispatch")
    owner, ws, project = chain["owner"], chain["workspace"], chain["project"]
    seed_helpers.workspace_member(db_conn, str(ws["id"]), str(owner["id"]), role=15)
    seed_helpers.project_member(
        db_conn, str(project["id"]), str(ws["id"]), str(owner["id"]), role=15
    )
    seed = seed_helpers.Seed(db_conn)
    todo_id = seed.state(str(project["id"]), str(ws["id"]), name="Todo", group="unstarted")
    progress_id = seed.state(
        str(project["id"]), str(ws["id"]), name="In Progress", group="started"
    )
    issue_id = str(chain["issue"]["id"])
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE projects SET agent_ticking_enabled=TRUE, "
            "agent_default_interval_seconds=10800, "
            "agent_review_default_interval_seconds=10800, "
            "agent_test_default_interval_seconds=10800, "
            "agent_default_max_ticks=20, default_agent_executor='local_runner' "
            "WHERE id = %s",
            (str(project["id"]),),
        )
        cur.execute(
            "UPDATE issues SET state_id = %s WHERE id = %s",
            (str(todo_id), issue_id),
        )
    pod_id = seed.pod(str(ws["id"]), str(project["id"]))
    seed.runner(str(owner["id"]), str(ws["id"]), str(pod_id))
    seed.override(str(ws["id"]), BROKEN_OVERRIDE_SECTION, BROKEN_OVERRIDE_BODY)
    db_conn.commit()

    stub = _ToolCallStub(issue_id, str(progress_id))
    base = stub.start()
    try:
        cookie = login_session_cookie(_base_url(), owner["email"], PASSWORD)
        client = api_client(_base_url(), cookie)
        _configure_llm(client, base)

        threads_base = f"/api/workspaces/{ws['slug']}/ai-assistant"
        res = client.post(f"{threads_base}/threads/", json={"title": "dispatch it"})
        assert res.status_code == 201, res.text
        thread_id = res.json()["id"]
        res = client.post(
            f"{threads_base}/threads/{thread_id}/messages/",
            json={"content": "Please start a coding run on the issue."},
        )
        assert res.status_code == 202, res.text
        turn_id = res.json()["turn"]["id"]

        row = wait_for(
            lambda: _turn_row(db_conn, turn_id)
            if _turn_row(db_conn, turn_id)["status"] == "completed"
            else None,
            what="tool-call turn completed",
            timeout=180,
        )
        assert row["status"] == "completed"

        # Django's dispatch_coding_run never saves the move: it calls
        # handle_issue_state_transition (which only reacts — ticker plus
        # entry run) and reports new_state, but the issue row stays in
        # Todo (runs.py:81-96 has no issue.save). Rust ports the same
        # react-only drive, so both backends leave the row put.
        assert str(_issue_state(db_conn, issue_id)) == str(todo_id), (
            "the dispatch path leaves the issue row in its prior state"
        )
        tickers = _ticker_rows(db_conn, issue_id)
        assert len(tickers) == 1, "the entered-bucket fire arms exactly one ticker"
        ticker = tickers[0]
        assert ticker["enabled"] is True
        assert ticker["disarm_reason"] == ""
        assert ticker["next_run_at"] is not None
        # PIDASHCONV-1589: Django stamps NULL here — dispatch_coding_run
        # never enters impersonate(user) (unlike issues.py create/update),
        # so BaseModel.save() sees no crum user. Rust must match.
        assert ticker["created_by_id"] is None, (
            "the dispatch path stamps no crum user on the entry ticker"
        )
        runs = _run_rows(db_conn, issue_id)
        assert len(runs) == 1, "the failed prompt build still commits its run row"
        run = runs[0]
        assert run["status"] == "failed"
        assert run["error_details"]["code"] == "prompt_build_failed"
        assert run["trigger"] == "state_transition"
        assert run["phase_kind"] == "coding-task"
        assert run["error_details"]["message"].startswith("prompt build failed:")
        assert set(run["error_details"]) == {"code", "message"}
        assert run["refusal_category"] == ""
        results = _tool_results(stub.payloads)
        assert len(results) == 1, "the follow-up carries exactly one tool result"
        result = results[0]
        assert result["dispatched"] is True
        assert result["run_id"] == str(run["id"])
        assert result["new_state"] == "In Progress"
        assert result["reason"] == "render-failed"

        def _failed_comment():
            comments = _comment_rows(db_conn, issue_id)
            if len(comments) == 1:
                return comments[0]
            return None

        # The failed run's terminal effects post one "Run failed" comment
        # (its tail is the template engine's own render error, so only the
        # engine-independent head is pinned). Django applies the effects
        # inline; Rust enqueues them for its worker — poll for both.
        comment = wait_for(
            _failed_comment,
            what="failed-run comment posted",
            timeout=180,
        )
        stripped = comment["comment_stripped"]
        assert stripped.startswith("Run failed.")
        assert "prompt build failed" in stripped
        assert "default-posture" in stripped
    finally:
        stub.stop()
