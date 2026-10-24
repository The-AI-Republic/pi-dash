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
