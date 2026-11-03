# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""create_issue post_save orchestration oracle (PIDASHCONV-1251).

The assistant ``create_issue`` tool saves through ``Issue.save()``
(``assistant/tools/issues.py:256-277``), so the ``post_save`` receiver
(``orchestration/signals.py:74-108``) fires with prior state None and the
saved state: the clock arms and an entry run may dispatch immediately.

Black-box contract, task-oracle style: seed a project with a ticking state
(``In Progress``), a turn trio and a scripted OpenAI stub whose first reply
is a ``create_issue`` tool call into that state; publish
``assistant.run_turn`` in Celery wire format; diff the orchestration rows.
Three dispatch shapes, all asserted row-for-row:

- no pod: the ticker arms, no run (``no-pod-available``);
- pod without a runner: the bounce comment posts, the ticker disarms
  explicitly (no safe Backlog target), the issue stays, no run;
- pod with a user-owned runner: the ticker arms and one queued entry run
  dispatches immediately.

PIDASHCONV-1444 adds a fourth shape over the same driver: pod without a
runner but WITH a Backlog state seeded, so the bounce moves the row to
Backlog. The oracle pins the DB move and the tool-result JSON together —
Django's bounce mutates the in-memory issue, so the tool result reports
the Backlog state, not the requested ticking state.

The Rust mirror lives in ``tasks_assistant/test_rust_replay.py`` (same
cases over ``rust_job_queue``). Live tests need the task-oracle stack:
Django + a Celery worker on ``CELERY_BROKER_URL``, Postgres at
``DATABASE_URL``, the server at ``BASE_URL`` (BYOK config rides the
public endpoint), and Fernet BYOK crypto
(``ASSISTANT_ENCRYPTION_KEY``).
"""

from __future__ import annotations

import json
import os
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from psycopg.rows import dict_row

from _harness import celery_wire
from _harness import seed as seed_helpers
from _harness.auth import api_client, login_session_cookie
from _harness.db import wait_for_condition as wait_for

RUN_TURN = "assistant.run_turn"

PASSWORD = "contract-suite-password"

TICKING_STATE = "In Progress"
TICKING_GROUP = "started"

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


# --------------------------------------------------------------------------- #
# Live oracle helpers
# --------------------------------------------------------------------------- #


def _base_url() -> str:
    return os.environ["BASE_URL"]


def _now_iso() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def _seed_member(db_conn, tag: str):
    """One user + workspace + membership; returns (user, workspace)."""
    seed_helpers.ensure_setup_done(db_conn)
    user = seed_helpers.user(db_conn, f"create-transition-{tag}")
    workspace = seed_helpers.workspace(db_conn, f"create-transition-{tag}", user["id"])
    seed_helpers.workspace_member(db_conn, workspace["id"], user["id"], role=15)
    return user, workspace


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


def _seed_turn_trio(db_conn, user, workspace):
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
            (thread_id, str(workspace["id"]), str(user["id"]), "create-transition", now, now),
        )
        cur.execute(
            "INSERT INTO assistant_turn (id, thread_id, status, model_used, error_code, "
            "error_detail, created_at, started_at) "
            "VALUES (%s, %s, 'queued', '', '', '', %s, NULL)",
            (turn_id, thread_id, now),
        )
        cur.execute(
            "INSERT INTO assistant_message "
            "(id, thread_id, turn_id, seq, kind, display_content, payload, status, created_at) "
            "VALUES (%s, %s, %s, 1, 'user', 'file it', '{}', 'completed', %s)",
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


# --------------------------------------------------------------------------- #
# Provider stub: first reply is a tool call, then text
# --------------------------------------------------------------------------- #


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


# --------------------------------------------------------------------------- #
# The oracle: one driven turn per dispatch shape
# --------------------------------------------------------------------------- #


def _drive_create_issue(db_conn, broker_url, tag, *, pod=False, runner=False, backlog=False,
                        broken_prompt=False):
    """Seed, drive one ``create_issue`` tool call, wait for the terminal
    turn. Returns (user, project_id, state_id, pod_id, turn_id, requests),
    where requests are the provider stub's captured payloads (the
    follow-up request carries the tool result). ``backlog`` seeds one
    Backlog state so the bounce has a target (PIDASHCONV-1444).
    ``broken_prompt`` seeds a workspace override whose body fails to parse,
    so the entry-run prompt build raises inside the post-save orchestration
    (PIDASHCONV-1571)."""
    user, workspace = _seed_member(db_conn, tag)
    project_id, state_id = _seed_ticking_project(db_conn, user, workspace)
    if backlog:
        seed_helpers.Seed(db_conn).state(
            project_id, workspace["id"], name="Backlog", group="backlog"
        )
        db_conn.commit()
    if broken_prompt:
        seed_helpers.Seed(db_conn).override(
            str(workspace["id"]), BROKEN_OVERRIDE_SECTION, BROKEN_OVERRIDE_BODY
        )
        db_conn.commit()
    pod_id = _seed_pod(db_conn, workspace, project_id) if pod else None
    if runner:
        assert pod_id is not None
        _seed_runner(db_conn, user, workspace, pod_id)
    cookie = login_session_cookie(_base_url(), user["email"], PASSWORD)
    client = api_client(_base_url(), cookie)
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
        _configure_llm(client, base)
        celery_wire.publish(RUN_TURN, args=[turn_id], broker_url=broker_url)
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


def _terminal_turn(db_conn, turn_id):
    row = _turn_row(db_conn, turn_id)
    db_conn.commit()
    if row and row["status"] in ("completed", "failed", "cancelled"):
        return row
    return None


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


def test_create_issue_arms_ticker_without_pod(db_conn, broker_url):
    """No pod: the fire arms the clock, no entry run (``no-pod-available``)."""
    user, project_id, state_id, _pod_id, _turn_id, _requests = _drive_create_issue(
        db_conn, broker_url, "nopod"
    )
    issue = _assert_tool_created_issue(db_conn, project_id, state_id, "nopod")
    _assert_ticker_armed(db_conn, issue, user["id"])
    assert _run_rows(db_conn, issue["id"]) == []
    assert _comment_rows(db_conn, issue["id"]) == []


def test_create_issue_bounces_without_runner(db_conn, broker_url):
    """Pod without a runner: the bounce posts, the ticker disarms, no run.

    No Backlog state and no safe fallback exist, so the issue stays in
    place and the ticker is disarmed explicitly (``scheduling.py``
    ``_bounce_issue_no_eligible_runner``).
    """
    user, project_id, state_id, _pod_id, _turn_id, _requests = _drive_create_issue(
        db_conn, broker_url, "bounce", pod=True
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


def test_create_issue_dispatches_entry_run(db_conn, broker_url):
    """Pod with a user-owned runner: an entry run dispatches immediately."""
    user, project_id, state_id, pod_id, _turn_id, _requests = _drive_create_issue(
        db_conn, broker_url, "dispatch", pod=True, runner=True
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
    assert run["error_details"] == {}
    assert run["refusal_category"] == ""
    assert run["dispatch_attempts"] == 0
    assert run["run_config"] == {"repo_ref": None, "repo_url": None, "git_work_branch": None}
    assert run["required_capabilities"] == []
    assert run["tool_plan"] == {}
    assert run["agent_metadata"] == {}
    assert "transition probe dispatch" in run["prompt"]
    assert _run_event_count(db_conn, run["id"]) == 0
    assert _comment_rows(db_conn, issue["id"]) == []


def test_create_issue_bounce_reports_backlog_tool_result(db_conn, broker_url):
    """Bounce to a Backlog target: the DB row AND the tool result say Backlog.

    PIDASHCONV-1444: Django's bounce mutates the in-memory issue
    (``scheduling.py:1111-1113``), so the tool result built after the
    atomic block (``issues.py:284``) reports the Backlog state — not the
    requested ticking state. The sibling no-target bounce above stays in
    place and cannot see the difference; this case seeds the target.
    """
    user, project_id, _state_id, _pod_id, _turn_id, requests = _drive_create_issue(
        db_conn, broker_url, "bouncereport", pod=True, backlog=True
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


def test_create_issue_prompt_failure_completes_turn(db_conn, broker_url):
    """Broken prompt template: post-insert orchestration fails, tool still briefs.

    PIDASHCONV-1571: the entry-run prompt build raises inside
    ``handle_issue_state_transition``. Django's ``post_save`` receiver
    swallows every handler error (``orchestration/signals.py:98-107``), so
    the save stands and the tool returns its normal brief: the issue
    exists, the ticker stays armed, the entry run row commits as
    failed/``prompt_build_failed``, and the turn completes.
    """
    user, project_id, state_id, _pod_id, _turn_id, requests = _drive_create_issue(
        db_conn, broker_url, "promptfail", pod=True, runner=True, broken_prompt=True
    )
    issue = _assert_tool_created_issue(db_conn, project_id, state_id, "promptfail")
    _assert_ticker_armed(db_conn, issue, user["id"])
    runs = _run_rows(db_conn, issue["id"])
    assert len(runs) == 1, "the failed prompt build still commits its run row"
    run = runs[0]
    assert run["status"] == "failed"
    assert run["error_details"]["code"] == "prompt_build_failed"
    assert run["trigger"] == "state_transition"
    assert run["error_details"]["message"].startswith("prompt build failed:")
    assert set(run["error_details"]) == {"code", "message"}
    assert run["refusal_category"] == ""
    result = _tool_result(requests)
    assert result["created"] is True
    assert str(result["id"]) == str(issue["id"])
    assert result["state"] == TICKING_STATE
    assert result["state_group"] == TICKING_GROUP
    # The failed run's terminal effects post one "Run failed" comment
    # (``run_lifecycle._post_failure_comment``). Its tail is the template
    # engine's own render error, so only the engine-independent head is
    # pinned here.
    comments = _comment_rows(db_conn, issue["id"])
    assert len(comments) == 1, "the failed run posts exactly one comment"
    stripped = comments[0]["comment_stripped"]
    assert stripped.startswith("Run failed.")
    assert "prompt build failed" in stripped
    assert "default-posture" in stripped
