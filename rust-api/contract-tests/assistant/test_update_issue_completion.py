# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Assistant update_issue → completion comment-back (PIDASHCONV-1252).

Django fires ``post_completion_comment`` from the ``github_signals``
post_save receiver (``bgtasks/github_signals.py:44-75``) on every Issue
save that transitions into a ``completed``-group state. The assistant
``update_issue`` tool saves through ``Issue.save()``
(``assistant/tools/issues.py:369``), so the signal fires and a synced
issue closed through the assistant gets its upstream completion comment.

Live oracle: seed an issue in a non-completed state with a
``GitIssueSync`` row whose metadata carries no ``completion_comment_id``,
run a turn whose model emits a single ``update_issue`` tool call moving
it to a Done-group state, and prove the comment task ran: the worker
executes it with the seeded invalid token, the provider POST fails, and
the sync metadata records ``completion_comment_error``
(``git_sync_task.py:304-318`` — every in-try fault records, including
auth and network failures, so the assertion holds with or without
provider network access).

Needs a worker on both backends (the turn and the comment task both
execute there), so this file is listed in ``TASK_ORACLE_IGNORES.txt``
like ``test_tasks_turn.py``. Backend-agnostic: the turn is triggered
through the message POST (both backends enqueue ``assistant.run_turn``
from it) and the assertion polls Postgres.

PIDASHCONV-1444 adds a second update_issue shape over the same driver:
a move into a ticking state that bounces to a Backlog target. The DB
row AND the tool result must say Backlog — Django's bounce mutates the
in-memory issue the result briefs (``issues.py:388``).
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
from integrations import seed as int_seed

PASSWORD = "contract-suite-password"

BOUNCE_STRIPPED = (
    "Agent run skipped \u2014 no eligible runner."
    "No runner is registered in this pod that can serve this issue. "
    "Add a runner under your account, or assign this issue to a workspace "
    "member whose runner is registered here."
)

#: Override body that fails to parse on both template engines (Jinja2 on
#: Django, minijinja strict on Rust), so the entry-run prompt build raises
#: inside the post-save orchestration (PIDASHCONV-1571). ``default-posture``
#: is an ``overridable`` coding-task section, so a workspace-level row wins
#: over the registry default on both backends.
BROKEN_OVERRIDE_SECTION = "default-posture"
BROKEN_OVERRIDE_BODY = "{% if %}\n"


def _base_url() -> str:
    return os.environ["BASE_URL"]


def _database_url() -> str:
    return os.environ["DATABASE_URL"]


def _now_iso() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


class _ToolCallStub:
    """OpenAI-compatible stub whose first completion is one tool call.

    Serves ``POST /v1/chat/completions`` in streaming and non-streaming
    form. Until a ``role: tool`` message appears in the request history
    (both the Django pydantic-ai lane and the Rust turn lane send tool
    results back as OpenAI ``tool``-role messages), the stub answers
    with a single ``update_issue`` call; afterwards it streams a short
    final text so the turn completes.
    """

    def __init__(self, issue_id: str, state_id: str):
        self.arguments = json.dumps({"issue_id": issue_id, "state_id": state_id})
        self.requests: list[dict] = []
        #: Full request payloads (PIDASHCONV-1444): the follow-up request's
        #: ``role: tool`` message carries the tool-result JSON.
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


def _sync_metadata(db_conn, sync_id):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT metadata FROM git_issue_syncs WHERE id = %s", (sync_id,))
        row = cur.fetchone()
        return row["metadata"] if row else None


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


def test_update_issue_to_completed_enqueues_post_completion_comment(db_conn):
    """A turn moving a synced issue to Done runs the completion comment.

    Seeds an issue in a started state mirrored by a ``GitIssueSync``
    row with empty metadata, drives one turn whose model emits
    ``update_issue(issue_id, state_id=<Done>)``, then proves the
    comment task executed: the sync metadata records the provider
    failure (the seeded token is invalid, so success is impossible).
    Without the enqueue the metadata stays empty and the test fails.
    """
    seed_helpers.ensure_setup_done(db_conn)
    chain = seed_helpers.issue_chain(db_conn, "1252done")
    owner, ws, project = chain["owner"], chain["workspace"], chain["project"]
    seed_helpers.workspace_member(db_conn, str(ws["id"]), str(owner["id"]), role=15)
    seed_helpers.project_member(
        db_conn, str(project["id"]), str(ws["id"]), str(owner["id"]), role=15
    )
    started = seed_helpers.state(
        db_conn, str(project["id"]), str(ws["id"]), name="Doing", group="started"
    )
    done = seed_helpers.state(
        db_conn, str(project["id"]), str(ws["id"]), name="Done", group="completed"
    )
    issue_id = str(chain["issue"]["id"])
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET state_id = %s WHERE id = %s",
            (str(started["id"]), issue_id),
        )

    database_url = _database_url()
    anchor = {
        "workspace": {"id": str(ws["id"])},
        "projects": [{"id": str(project["id"]), "workspace_id": str(ws["id"])}],
        "actor": {"id": str(owner["id"])},
    }
    scope = int_seed.Scope(database_url)
    unique = uuid.uuid4().hex[:8]
    account_id = int_seed.provider_account(database_url, anchor, scope)
    repo_id = int_seed.repository(database_url, scope, suffix=f"ct1252-{unique}")
    binding_id = int_seed.binding(
        database_url, anchor, scope, account_id=account_id, repository_id=repo_id
    )
    sync_id = int_seed.git_issue_sync(
        database_url,
        anchor,
        scope,
        binding_id=binding_id,
        issue_id=issue_id,
        external_iid=f"ct-1252-{unique}",
        metadata={},
    )

    stub = _ToolCallStub(issue_id, str(done["id"]))
    base = stub.start()
    try:
        cookie = login_session_cookie(_base_url(), owner["email"], PASSWORD)
        client = api_client(_base_url(), cookie)
        _configure_llm(client, base)

        threads_base = f"/api/workspaces/{ws['slug']}/ai-assistant"
        res = client.post(f"{threads_base}/threads/", json={"title": "close it"})
        assert res.status_code == 201, res.text
        thread_id = res.json()["id"]
        res = client.post(
            f"{threads_base}/threads/{thread_id}/messages/",
            json={"content": "Please mark the issue as done."},
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

        assert str(_issue_state(db_conn, issue_id)) == str(done["id"]), (
            "the turn's update_issue call must move the issue to Done"
        )

        def _comment_attempted():
            metadata = _sync_metadata(db_conn, sync_id) or {}
            if metadata.get("completion_comment_id") or metadata.get(
                "completion_comment_error"
            ):
                return metadata
            return None

        metadata = wait_for(
            _comment_attempted,
            what="post_completion_comment executed",
            timeout=180,
        )
        assert metadata.get("completion_comment_id") or metadata.get(
            "completion_comment_error"
        ), f"sync metadata never recorded the comment attempt: {metadata!r}"
    finally:
        stub.stop()


def test_update_issue_bounce_reports_backlog_tool_result(db_conn):
    """Bounce to a Backlog target: the DB row AND the tool result say Backlog.

    PIDASHCONV-1444: a turn moves a Todo issue into a ticking state with a
    pod but no runner, so the drive bounces it to Backlog. Django's bounce
    mutates the in-memory issue the result briefs (``issues.py:388``), so
    the tool result reports the Backlog state — not the requested ticking
    state. Backend-agnostic like the sibling test: the same assertions run
    against Django and Rust.
    """
    seed_helpers.ensure_setup_done(db_conn)
    chain = seed_helpers.issue_chain(db_conn, "1444bounce")
    owner, ws, project = chain["owner"], chain["workspace"], chain["project"]
    seed_helpers.workspace_member(db_conn, str(ws["id"]), str(owner["id"]), role=15)
    seed_helpers.project_member(
        db_conn, str(project["id"]), str(ws["id"]), str(owner["id"]), role=15
    )
    seed = seed_helpers.Seed(db_conn)
    todo_id = seed.state(str(project["id"]), str(ws["id"]), name="Todo", group="unstarted")
    # The ticking gate pins the literal state name per group
    # (``agent_phases.is_ticking_state``) — a custom name would not trigger.
    progress_id = seed.state(
        str(project["id"]), str(ws["id"]), name="In Progress", group="started"
    )
    backlog_id = chain["state"]["id"]
    issue_id = str(chain["issue"]["id"])
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE states SET name = 'Backlog' WHERE id = %s",
            (str(backlog_id),),
        )
        cur.execute(
            "UPDATE issues SET state_id = %s WHERE id = %s",
            (str(todo_id), issue_id),
        )
    seed.pod(str(ws["id"]), str(project["id"]))
    db_conn.commit()

    stub = _ToolCallStub(issue_id, str(progress_id))
    base = stub.start()
    try:
        cookie = login_session_cookie(_base_url(), owner["email"], PASSWORD)
        client = api_client(_base_url(), cookie)
        _configure_llm(client, base)

        threads_base = f"/api/workspaces/{ws['slug']}/ai-assistant"
        res = client.post(f"{threads_base}/threads/", json={"title": "move it"})
        assert res.status_code == 201, res.text
        thread_id = res.json()["id"]
        res = client.post(
            f"{threads_base}/threads/{thread_id}/messages/",
            json={"content": "Please start work on the issue."},
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

        assert str(_issue_state(db_conn, issue_id)) == str(backlog_id), (
            "the bounce moves the row to Backlog"
        )
        results = _tool_results(stub.payloads)
        assert len(results) == 1, "the follow-up carries exactly one tool result"
        result = results[0]
        assert result["updated"] is True
        assert str(result["id"]) == issue_id
        assert result["changed"] == ["state"]
        assert result["state"] == "Backlog"
        assert result["state_group"] == "backlog"
        tickers = _ticker_rows(db_conn, issue_id)
        assert len(tickers) == 1
        ticker = tickers[0]
        assert ticker["enabled"] is False
        assert ticker["disarm_reason"] == "left_ticking_state"
        assert ticker["next_run_at"] is None
        assert str(ticker["created_by_id"]) == str(owner["id"])
        comments = _comment_rows(db_conn, issue_id)
        assert len(comments) == 1, "the bounce posts exactly one comment"
        assert comments[0]["comment_stripped"] == BOUNCE_STRIPPED
        assert _run_rows(db_conn, issue_id) == []
    finally:
        stub.stop()


def test_update_issue_prompt_failure_commits_move(db_conn):
    """Prompt build failure mid-drive: the edit commits, the tool briefs.

    PIDASHCONV-1571: a turn moves a Todo issue into a ticking state with a
    pod and an eligible runner, but the entry-run prompt build raises
    (broken workspace override). The prompt path finalizes the run FAILED
    and returns ``render-failed`` with no exception
    (``orchestration/service.py:820-832``), so Django commits the whole
    atomic and the tool returns its normal brief — with
    ``dispatched_run_id`` pointing at the failed run. Backend-agnostic
    like the sibling tests.
    """
    seed_helpers.ensure_setup_done(db_conn)
    chain = seed_helpers.issue_chain(db_conn, "1571upfail")
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
        res = client.post(f"{threads_base}/threads/", json={"title": "move it"})
        assert res.status_code == 201, res.text
        thread_id = res.json()["id"]
        res = client.post(
            f"{threads_base}/threads/{thread_id}/messages/",
            json={"content": "Please start work on the issue."},
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

        assert str(_issue_state(db_conn, issue_id)) == str(progress_id), (
            "the move commits although the prompt build failed"
        )
        tickers = _ticker_rows(db_conn, issue_id)
        assert len(tickers) == 1, "the entered-bucket fire arms exactly one ticker"
        ticker = tickers[0]
        assert ticker["enabled"] is True
        assert ticker["disarm_reason"] == ""
        assert ticker["next_run_at"] is not None
        assert str(ticker["created_by_id"]) == str(owner["id"])
        runs = _run_rows(db_conn, issue_id)
        assert len(runs) == 1, "the failed prompt build still commits its run row"
        run = runs[0]
        assert run["status"] == "failed"
        assert run["error_code"] == "prompt_build_failed"
        assert run["trigger"] == "state_transition"
        assert run["phase_kind"] == "coding-task"
        assert run["error"].startswith("prompt build failed:")
        results = _tool_results(stub.payloads)
        assert len(results) == 1, "the follow-up carries exactly one tool result"
        result = results[0]
        assert result["updated"] is True
        assert str(result["id"]) == issue_id
        assert result["changed"] == ["state"]
        assert result["state"] == "In Progress"
        assert result["state_group"] == "started"
        assert result["dispatched_run_id"] == str(run["id"])

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
