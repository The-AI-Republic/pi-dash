# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Oracle: cloud-agent create fires the post_save transition (PIDASHCONV-1493).

Django's ``pidash_create_project_issue`` saves through
``Issue.objects.create`` (``cloud_agent/tools.py:345-371``), so the
``post_save`` receiver (``orchestration/signals.py:74-107``) fires with
prior state None and the resolved state: creating directly into a
ticking default state arms the clock and dispatches the entry run,
exactly as a human create does.

Live oracle: seed a project whose ``default`` state is the ticking
``In Progress`` state (delegation configured: ticking flags, a default
pod, a user-owned runner, creator BYOK row), run a scheduler-bound
cloud-agent run whose model emits a single
``pidash_create_project_issue`` call, and prove the fire: the issue
lands in the ticking state, its ticker row is armed, and exactly one
``state_transition`` entry run exists for it. The negative case seeds a
``Backlog`` default (with a non-default ticking state present, so the
tool demonstrably resolves the default): the issue is created, and no
ticker, run, or comment appears. Two more cases pin the fire's
post-commit shape: an online idle runner takes the entry run straight
to ``assigned`` (the inline pod drain, PIDASHCONV-1494), and a
runnerless pod bounces the issue back to Backlog with the
no-eligible-runner notice and a dormant ticker.

Needs a worker on both backends (the scheduler run executes there).
Backend selection follows
``dispatch/test_transition_completion_comment.py``:
``PIDASH_CONTRACT_BACKEND`` ``=django`` (default) publishes Celery wire
v2 to the Redis broker (a Django celery worker must be running);
``=rust`` seeds ``rust_job_queue`` (``pidash-api worker`` must be
running against the same ``DATABASE_URL``). No HTTP server is needed:
the LLM config row is seeded directly with a Fernet-encrypted key
(``tasks_assistant/test_rust_replay.py`` pattern), so the workers need
``CLOUD_AGENT_ENABLED=true``, ``CLOUD_AGENT_WRITES_ENABLED=true`` (the
create tool is a write), ``CLOUD_AGENT_BLOCK_PRIVATE_URLS=false`` (the
stub is loopback), ``ASSISTANT_CRYPTO_BACKEND=fernet`` and the matching
``ASSISTANT_ENCRYPTION_KEY``.
"""

from __future__ import annotations

import datetime
import json
import os
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest
from cryptography.fernet import Fernet

from _harness import rust_queue
from _harness.db import wait_for_condition as wait_for
from app_scheduler import seed_scheduler as sched_seed

from .conftest import (
    COMPLETED,
    create_agent_run,
    wait_for_queue_drain,
    wait_for_run,
)

pytestmark = pytest.mark.contract

BACKEND = os.environ.get("PIDASH_CONTRACT_BACKEND", "django")

RUN_TASK = "cloud_agent.run_agent_run"

CREATE_TOOL = "pidash_create_project_issue"

TICKING_STATE = "In Progress"
TICKING_GROUP = "started"

#: Scheduler-run tool plan (``policy.build_tool_plan(run_kind="scheduler",
#: has_issue=False)`` with writes enabled, minus the github tools, which
#: need a verified GitHub App installation). The create tool is
#: scheduler-only, so it must be in the seeded plan — the runtime only
#: re-intersects, never adds (``policy.resolve_current_tool_names``).
PLAN_TOOLS = sorted(
    [
        "pidash_list_project_states",
        "pidash_search_project_issues",
        "pidash_get_project_issue",
        "pidash_list_issue_relations",
        "pidash_create_project_issue",
        "pidash_relate_issues",
        "pidash_unrelate_issues",
    ]
)

FINAL_RESULT_ARGS = {
    "outcome": "completed",
    "summary": "Done",
    "evidence": [],
    "limitations": [],
}

RUN_TIMEOUT = 180.0
EFFECT_TIMEOUT = 180.0
QUIESCE_SECONDS = 10.0


class _CreateStub:
    """OpenAI-compatible stub whose first completion is one create call.

    Serves ``POST /v1/chat/completions`` in streaming and non-streaming
    form. The first request ever gets a single
    ``pidash_create_project_issue`` call; every later request calls the
    ``final_result`` output tool so the run ends. Emitting the create
    exactly once (rather than per tool-history) also keeps a stray
    second model session from filing a second issue.
    """

    def __init__(self, title: str):
        self.create_arguments = json.dumps({"title": title, "description": ""})
        self.final_arguments = json.dumps(FINAL_RESULT_ARGS)
        self.requests: list[dict] = []
        self.create_emitted = 0
        self._server = None
        self._thread = None
        self._lock = threading.Lock()

    def start(self) -> str:
        stub = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.0"

            def log_message(self, *args):
                pass

            def _reply(self, payload):
                with stub._lock:
                    first = stub.create_emitted == 0
                    stub.requests.append(
                        {
                            "path": self.path,
                            "stream": payload.get("stream"),
                            "first": first,
                        }
                    )
                    if first:
                        stub.create_emitted += 1
                if first:
                    return (
                        "call_create_1",
                        CREATE_TOOL,
                        stub.create_arguments,
                        "tool_calls",
                    )
                return "call_final_1", "final_result", stub.final_arguments, "stop"

            def do_POST(self):
                length = int(self.headers.get("Content-Length", 0))
                body = self.rfile.read(length) if length else b""
                try:
                    payload = json.loads(body or b"{}")
                except ValueError:
                    payload = {}
                call_id, name, arguments, finish = self._reply(payload)
                if self.path.endswith("/chat/completions") and payload.get("stream"):
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream")
                    self.end_headers()
                    self.wfile.write(
                        (
                            '{"id":"stub","object":"chat.completion.chunk",'
                            '"created":0,"model":"oracle-stub","choices":[{"index":0,'
                            '"delta":{"role":"assistant","content":null,'
                            '"tool_calls":[{"index":0,"id":"'
                            + call_id
                            + '","type":"function","function":{"name":"'
                            + name
                            + '","arguments":'
                            + json.dumps(arguments)
                            + '}}]},"finish_reason":null}]}\n\n'
                        ).encode()
                    )
                    self.wfile.write(
                        (
                            '{"id":"stub","object":"chat.completion.chunk",'
                            '"created":0,"model":"oracle-stub","choices":[{"index":0,'
                            '"delta":{},"finish_reason":"'
                            + finish
                            + '"}]}\n\n'
                        ).encode()
                    )
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
                                    "message": {
                                        "role": "assistant",
                                        "content": None,
                                        "tool_calls": [
                                            {
                                                "id": call_id,
                                                "type": "function",
                                                "function": {
                                                    "name": name,
                                                    "arguments": arguments,
                                                },
                                            }
                                        ],
                                    },
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


def _seed_llm_config(seeder, user_id: str, base_url: str) -> None:
    """Seed a decryptable BYOK row pointing at the stub (no HTTP needed).

    Both workers decrypt ``api_key_encrypted`` under the Fernet backend
    (``ASSISTANT_CRYPTO_BACKEND=fernet`` +
    ``ASSISTANT_ENCRYPTION_KEY``), so the test encrypts the dummy key
    itself, like ``tasks_assistant/test_rust_replay.py``.
    """
    key = os.environ["ASSISTANT_ENCRYPTION_KEY"].encode("utf-8")
    token = Fernet(key).encrypt(b"test-key-123")
    seeder.db.execute(
        "INSERT INTO assistant_user_llm_config "
        "(user_id, provider_kind, base_url, model_name, api_key_encrypted, "
        "created_at, updated_at) "
        "VALUES (%s, 'openai_compatible', %s, 'oracle-stub', %s, now(), now())",
        (user_id, base_url, token),
    )
    row_id = seeder.db.fetchval(
        "SELECT id FROM assistant_user_llm_config WHERE user_id = %s", (user_id,)
    )
    seeder.track.add("assistant_user_llm_config", row_id)


def _seed_ticking_project(seeder, project_id: str) -> None:
    """Ticking flags + a runnable executor (``assistant/`` oracle shape).

    The seed default executor (``""``) is not runnable; ``local_runner``
    routes the entry run down the pod/runner path, where the seeded
    runner admits it and the run stays visibly queued (no local runner
    polls in this environment).
    """
    seeder.db.execute(
        "UPDATE projects SET agent_ticking_enabled=TRUE, "
        "agent_default_interval_seconds=10800, "
        "agent_review_default_interval_seconds=10800, "
        "agent_test_default_interval_seconds=10800, "
        "agent_default_max_ticks=20, default_agent_executor='local_runner' "
        "WHERE id = %s",
        (project_id,),
    )


def _seed_default_pod(seeder, workspace_id: str, project_id: str, owner_id: str) -> dict:
    """The project's default pod (the create tool resolves ``is_default``)."""
    pod_id = str(uuid.uuid4())
    seeder.db.execute(
        """INSERT INTO pod
           (id, name, description, is_default, deleted_at, created_at, updated_at,
            created_by_id, workspace_id, project_id)
           VALUES (%s,'Contract default pod','',true,NULL,now(),now(),%s,%s,%s)""",
        (pod_id, owner_id, workspace_id, project_id),
    )
    seeder.track.add("pod", pod_id)
    return {"id": pod_id}


def _seed_runner(
    seeder, owner_id: str, workspace_id: str, pod_id: str, *, online: bool = False
) -> dict:
    """A user-owned runner in the pod (offline is eligible for preflight).

    ``online=True`` seeds the drain-test shape
    (``orchestration/test_dispatch_drain.py::_fresh_runner``): a fresh
    heartbeat plus an active session row, so the inline pod drain
    assigns the entry run and the Assign enqueue hits the live stream.
    """
    runner_id = str(uuid.uuid4())
    name = f"ct1493-runner-{uuid.uuid4().hex[:6]}"
    now = datetime.datetime.now(datetime.timezone.utc).isoformat()
    seeder.db.execute(
        """INSERT INTO runner
           (id, name, host_label, refresh_token_hash, refresh_token_fingerprint,
            refresh_token_generation, previous_refresh_token_hash,
            access_token_signing_key_version, enrollment_token_hash,
            enrollment_token_fingerprint, revoked_reason, dev_metadata,
            provisioning, capabilities, status, os, arch, runner_version,
            protocol_version, owner_id, workspace_id, pod_id, dev_machine_id,
            visibility, created_at, updated_at)
           VALUES (%s,%s,'','','',0,'',1,'','', '', '{}','manual','[]',
                   'offline','','','',1,%s,%s,%s,NULL,0,%s,%s)""",
        (runner_id, name, owner_id, workspace_id, pod_id, now, now),
    )
    seeder.track.add("runner", runner_id)
    if online:
        seeder.db.execute(
            "UPDATE runner SET last_heartbeat_at = now(), status = 'online'"
            " WHERE id = %s",
            (runner_id,),
        )
        session_id = str(uuid.uuid4())
        seeder.db.execute(
            "INSERT INTO runner_session (id, runner_id, protocol_version,"
            " created_at, last_seen_at, revoked_at, revoked_reason)"
            " VALUES (%s,%s,4,now(),now(),NULL,'')",
            (session_id, runner_id),
        )
        seeder.track.add("runner_session", session_id)
    return {"id": runner_id}


def _seed_scheduler_binding(seeder, workspace_id: str, project_id: str, owner_id: str) -> dict:
    """One scheduler install on the project; disabled so beat never fires it.

    Run execution does not consult ``enabled`` — only the beat fire loop
    does — so a disabled binding keeps the drive to exactly the seeded run.
    """
    unique = uuid.uuid4().hex[:8]
    sched = sched_seed.create_scheduler(
        seeder.db,
        workspace_id=workspace_id,
        slug=f"ct1493-{unique}",
        name=f"ct1493-{unique}",
        created_by_id=owner_id,
    )
    seeder.track.add("schedulers", sched["id"])
    binding = sched_seed.create_binding(
        seeder.db,
        workspace_id=workspace_id,
        project_id=project_id,
        scheduler_id=sched["id"],
        dtstart=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        enabled=False,
        actor_id=owner_id,
    )
    seeder.track.add("scheduler_bindings", binding["id"])
    return binding


def _seed_scheduler_run(seeder, world, binding_id: str, pod_id: str) -> dict:
    """One QUEUED scheduler-bound cloud-agent run with a scheduler tool plan."""
    plan = {
        "v": 1,
        "catalog_version": 1,
        "tools": PLAN_TOOLS,
        "required_tools": [],
        "limits": {},
        "unavailable_capabilities": ["filesystem", "shell", "worktree"],
        "extra_toolsets": False,
    }
    run = create_agent_run(
        seeder,
        world["workspace"]["id"],
        pod_id,
        world["owner"]["id"],
        prompt="Please file an issue for the nightly scan findings.",
        tool_plan=json.dumps(plan),
    )
    seeder.db.execute(
        "UPDATE agent_run SET scheduler_binding_id = %s, trigger = 'scheduler' WHERE id = %s",
        (binding_id, run["id"]),
    )
    return run


def _publish(broker, run_id: str):
    """Publish one run invocation; returns the queue baseline for drains."""
    if BACKEND == "rust":
        rust_queue.ensure_schema()
        baseline = rust_queue.queue_depth()
        rust_queue.publish(RUN_TASK, [run_id], {})
        return baseline
    baseline = broker.queue_length()
    broker.publish(RUN_TASK, [run_id])
    return baseline


def _drain(broker, baseline):
    if BACKEND == "rust":
        rust_queue.wait_for_queue_drain(baseline=baseline)
    else:
        wait_for_queue_drain(broker, baseline=baseline)


def _issue_by_name(db, project_id: str, name: str):
    return db.fetchone(
        "SELECT * FROM issues WHERE project_id = %s AND name = %s", (project_id, name)
    )


def _ticker_rows(db, issue_id: str):
    return db.fetchall("SELECT * FROM issue_agent_ticker WHERE issue_id = %s", (issue_id,))


def _entry_runs(db, issue_id: str):
    return db.fetchall(
        "SELECT * FROM agent_run WHERE work_item_id = %s ORDER BY created_at", (issue_id,)
    )


def _comment_rows(db, issue_id: str):
    return db.fetchall(
        "SELECT * FROM issue_comments WHERE issue_id = %s ORDER BY created_at", (issue_id,)
    )


def _create_ledger_rows(db, run_id: str):
    return db.fetchall(
        "SELECT status FROM agent_run_tool_call "
        "WHERE agent_run_id = %s AND tool_name = %s",
        (run_id, CREATE_TOOL),
    )


def _track_side_effects(seeder, db, run_id: str, issue_id: str):
    """Track backend-created rows so teardown removes them (issue first, so
    the reverse-order cleanup deletes dependents before the issue; the
    scheduler run itself was tracked at seed time, so its ledger and
    events — added here — delete before it)."""
    seeder.track.add("issues", issue_id)
    for row in db.fetchall(
        "SELECT id FROM issue_sequences WHERE issue_id = %s", (issue_id,)
    ):
        seeder.track.add("issue_sequences", row["id"])
    for row in _ticker_rows(db, issue_id):
        seeder.track.add("issue_agent_ticker", row["id"])
    for row in _entry_runs(db, issue_id):
        seeder.track.add("agent_run", row["id"])
    for row in _comment_rows(db, issue_id):
        seeder.track.add("issue_comments", row["id"])
    for row in db.fetchall(
        "SELECT id FROM agent_run_tool_call WHERE agent_run_id = %s "
        "OR agent_run_id IN (SELECT id FROM agent_run WHERE work_item_id = %s)",
        (run_id, issue_id),
    ):
        seeder.track.add("agent_run_tool_call", row["id"])
    for row in db.fetchall(
        "SELECT id FROM agent_run_event WHERE agent_run_id = %s "
        "OR agent_run_id IN (SELECT id FROM agent_run WHERE work_item_id = %s)",
        (run_id, issue_id),
    ):
        seeder.track.add("agent_run_event", row["id"])


def test_create_tool_in_ticking_default_arms_ticker_and_dispatches_entry_run(
    db, broker, seeder, world
):
    """A scheduler-filed issue in a ticking default fires the transition.

    The project's ``default`` state is the ticking ``In Progress``
    state; one scheduler run emits ``pidash_create_project_issue``. The
    issue lands in the ticking state, its ticker is armed (enabled, no
    disarm reason, budget untouched, creator-stamped), and exactly one
    queued ``state_transition`` entry run exists for it — the same fire
    a human create gets. Without the post_save drive the issue sits
    idle: no ticker, no run.
    """
    owner, ws, project = world["owner"], world["workspace"], world["project"]
    _seed_ticking_project(seeder, project["id"])
    ticking = seeder.create_state(
        ws["id"], project["id"], name=TICKING_STATE, group=TICKING_GROUP
    )
    seeder.db.execute(
        'UPDATE states SET "default" = true WHERE id = %s', (ticking["id"],)
    )
    pod = _seed_default_pod(seeder, ws["id"], project["id"], owner["id"])
    _seed_runner(seeder, owner["id"], ws["id"], pod["id"])
    binding = _seed_scheduler_binding(seeder, ws["id"], project["id"], owner["id"])

    title = f"Scheduler-filed {uuid.uuid4().hex[:8]}"
    stub = _CreateStub(title)
    base = stub.start()
    try:
        _seed_llm_config(seeder, owner["id"], base)
        run = _seed_scheduler_run(seeder, world, binding["id"], pod["id"])

        _publish(broker, run["id"])
        after = wait_for_run(db, run["id"], status=COMPLETED, timeout=RUN_TIMEOUT)
        assert after["status"] == COMPLETED, (
            f"run ended {after['status']}/{after['error_code']}: {after['error']}"
        )

        ledger = _create_ledger_rows(db, run["id"])
        assert [row["status"] for row in ledger] == ["succeeded"], (
            f"the create tool must run exactly once: {ledger!r}; "
            f"stub saw {stub.create_emitted} create emission(s) "
            f"in {len(stub.requests)} request(s): {stub.requests!r}"
        )
        assert stub.create_emitted == 1, (
            f"the model must emit the create call exactly once: {stub.requests!r}"
        )

        issue = _issue_by_name(db, project["id"], title)
        assert issue is not None, "the tool call must create the issue"
        assert str(issue["state_id"]) == str(ticking["id"]), (
            "the issue must land in the ticking default state (no bounce move)"
        )
        assert issue["sequence_id"] == 1
        assert issue["created_via"] == "cloud_agent"
        assert str(issue["created_by_id"]) == str(owner["id"])
        _track_side_effects(seeder, db, run["id"], issue["id"])

        tickers = _ticker_rows(db, issue["id"])
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
        assert str(ticker["created_by_id"]) == str(owner["id"])

        def _runs_present():
            rows = _entry_runs(db, issue["id"])
            return rows or None

        runs = wait_for(
            _runs_present, timeout=EFFECT_TIMEOUT, what="entry run created"
        )
        assert len(runs) == 1, "the immediate dispatch creates exactly one entry run"
        entry = runs[0]
        assert entry["status"] == "queued"
        assert entry["trigger"] == "state_transition"
        assert entry["phase_kind"] == "coding-task"
        assert entry["executor_kind"] == "local_runner"
        assert str(entry["created_by_id"]) == str(owner["id"])
        assert str(entry["pod_id"]) == str(pod["id"])
        assert str(entry["work_item_id"]) == str(issue["id"])
        assert entry["parent_run_id"] is None
        assert entry["runner_id"] is None
        assert entry["pinned_runner_id"] is None
        assert entry["scheduler_binding_id"] is None
        assert entry["error"] == ""
        assert entry["error_code"] == ""
        assert entry["dispatch_attempts"] == 0
        base_branch = db.fetchone(
            "SELECT base_branch FROM projects WHERE id = %s", (project["id"],)
        )["base_branch"]
        assert entry["run_config"] == {
            "repo_ref": base_branch or None,
            "repo_url": None,
            "git_work_branch": None,
        }
        assert entry["required_capabilities"] == []
        assert entry["tool_plan"] == {}
        assert entry["agent_metadata"] == {}
        assert title in entry["prompt"]
        assert _comment_rows(db, issue["id"]) == []
    finally:
        stub.stop()


def test_create_tool_in_backlog_default_creates_neither(db, broker, seeder, world):
    """A scheduler-filed issue in a Backlog default stays idle.

    Same drive, but the ``default`` state is Backlog (a non-default
    ticking state is present so the tool demonstrably resolves the
    default): the issue is created in Backlog and no ticker, entry run,
    or comment may appear.
    """
    owner, ws, project = world["owner"], world["workspace"], world["project"]
    _seed_ticking_project(seeder, project["id"])
    backlog = seeder.create_state(ws["id"], project["id"], name="Backlog", group="backlog")
    seeder.db.execute(
        'UPDATE states SET "default" = true WHERE id = %s', (backlog["id"],)
    )
    seeder.create_state(ws["id"], project["id"], name=TICKING_STATE, group=TICKING_GROUP)
    pod = _seed_default_pod(seeder, ws["id"], project["id"], owner["id"])
    _seed_runner(seeder, owner["id"], ws["id"], pod["id"])
    binding = _seed_scheduler_binding(seeder, ws["id"], project["id"], owner["id"])

    title = f"Scheduler-filed {uuid.uuid4().hex[:8]}"
    stub = _CreateStub(title)
    base = stub.start()
    try:
        _seed_llm_config(seeder, owner["id"], base)
        run = _seed_scheduler_run(seeder, world, binding["id"], pod["id"])

        baseline = _publish(broker, run["id"])
        after = wait_for_run(db, run["id"], status=COMPLETED, timeout=RUN_TIMEOUT)
        assert after["status"] == COMPLETED, (
            f"run ended {after['status']}/{after['error_code']}: {after['error']}"
        )

        issue = _issue_by_name(db, project["id"], title)
        assert issue is not None, "the tool call must create the issue"
        assert str(issue["state_id"]) == str(backlog["id"]), (
            "the tool must resolve the Backlog default, not the ticking state"
        )
        _track_side_effects(seeder, db, run["id"], issue["id"])

        # Quiescence: an erroneous fire would already have written (the
        # signal runs inside the create), so a drained queue plus a
        # settle sleep proves silence.
        _drain(broker, baseline)
        time.sleep(QUIESCE_SECONDS)
        assert _ticker_rows(db, issue["id"]) == []
        assert _entry_runs(db, issue["id"]) == []
        assert _comment_rows(db, issue["id"]) == []
    finally:
        stub.stop()


def test_create_tool_with_online_runner_assigns_entry_run(db, broker, seeder, world):
    """The fire drains the pod: an online runner takes the entry run.

    Same ticking-default drive, but the pod's runner is online and
    idle (the ``test_dispatch_drain`` shape). Django's dispatch drains
    the pod inline (``cloud_agent/dispatch.py:61-70``), so the entry
    run must land ``assigned`` on that runner — not sit ``queued``.
    A fire that drops the ``PodDrain`` decision leaves it queued
    (PIDASHCONV-1494).
    """
    owner, ws, project = world["owner"], world["workspace"], world["project"]
    _seed_ticking_project(seeder, project["id"])
    ticking = seeder.create_state(
        ws["id"], project["id"], name=TICKING_STATE, group=TICKING_GROUP
    )
    seeder.db.execute(
        'UPDATE states SET "default" = true WHERE id = %s', (ticking["id"],)
    )
    pod = _seed_default_pod(seeder, ws["id"], project["id"], owner["id"])
    runner = _seed_runner(seeder, owner["id"], ws["id"], pod["id"], online=True)
    binding = _seed_scheduler_binding(seeder, ws["id"], project["id"], owner["id"])

    title = f"Scheduler-filed {uuid.uuid4().hex[:8]}"
    stub = _CreateStub(title)
    base = stub.start()
    try:
        _seed_llm_config(seeder, owner["id"], base)
        run = _seed_scheduler_run(seeder, world, binding["id"], pod["id"])

        _publish(broker, run["id"])
        after = wait_for_run(db, run["id"], status=COMPLETED, timeout=RUN_TIMEOUT)
        assert after["status"] == COMPLETED, (
            f"run ended {after['status']}/{after['error_code']}: {after['error']}"
        )

        ledger = _create_ledger_rows(db, run["id"])
        assert [row["status"] for row in ledger] == ["succeeded"], (
            f"the create tool must run exactly once: {ledger!r}; "
            f"stub saw {stub.create_emitted} create emission(s) "
            f"in {len(stub.requests)} request(s): {stub.requests!r}"
        )

        issue = _issue_by_name(db, project["id"], title)
        assert issue is not None, "the tool call must create the issue"
        assert str(issue["state_id"]) == str(ticking["id"]), (
            "the issue must land in the ticking default state (no bounce move)"
        )
        _track_side_effects(seeder, db, run["id"], issue["id"])

        tickers = _ticker_rows(db, issue["id"])
        assert len(tickers) == 1, "the entered-bucket fire arms exactly one ticker"
        assert tickers[0]["enabled"] is True

        def _runs_present():
            rows = _entry_runs(db, issue["id"])
            return rows or None

        runs = wait_for(
            _runs_present, timeout=EFFECT_TIMEOUT, what="entry run created"
        )
        assert len(runs) == 1, "the immediate dispatch creates exactly one entry run"
        entry = runs[0]
        # The drain pin: the idle online runner is offered the run
        # straight away (the fire awaits the drain before the tool
        # returns, so no settle sleep is needed).
        assert entry["status"] == "assigned", entry
        assert str(entry["runner_id"]) == str(runner["id"]), entry
        assert str(entry["owner_id"]) == str(owner["id"]), entry
        assert entry["assigned_at"] is not None
        assert entry["trigger"] == "state_transition"
        assert entry["executor_kind"] == "local_runner"
        assert str(entry["pod_id"]) == str(pod["id"])
        assert str(entry["work_item_id"]) == str(issue["id"])
        assert _comment_rows(db, issue["id"]) == []
    finally:
        stub.stop()


def test_create_tool_in_ticking_default_without_runner_bounces_to_backlog(
    db, broker, seeder, world
):
    """A runnerless pod bounces the scheduler-filed issue to Backlog.

    Ticking default, but the default pod has no runner, so the fire's
    preflight is ineligible
    (``orchestration/test_signal_dispatch.py::test_enter_bucket_bounces_to_backlog_when_pod_has_no_runner``):
    the issue moves to the project's Backlog state, one
    no-eligible-runner notice is posted, the ticker lands dormant
    (disabled, ``left_ticking_state``, no next run), and no entry run
    is created. A fire that holds the reconcile write in-transaction
    across the bounce commits an armed ticker instead.
    """
    owner, ws, project = world["owner"], world["workspace"], world["project"]
    _seed_ticking_project(seeder, project["id"])
    ticking = seeder.create_state(
        ws["id"], project["id"], name=TICKING_STATE, group=TICKING_GROUP
    )
    seeder.db.execute(
        'UPDATE states SET "default" = true WHERE id = %s', (ticking["id"],)
    )
    backlog = seeder.create_state(
        ws["id"], project["id"], name="Backlog", group="backlog"
    )
    # No runner is seeded: the bare pod makes the fire's preflight
    # ineligible, which is what the bounce pins.
    pod = _seed_default_pod(seeder, ws["id"], project["id"], owner["id"])
    binding = _seed_scheduler_binding(seeder, ws["id"], project["id"], owner["id"])

    title = f"Scheduler-filed {uuid.uuid4().hex[:8]}"
    stub = _CreateStub(title)
    base = stub.start()
    try:
        _seed_llm_config(seeder, owner["id"], base)
        run = _seed_scheduler_run(seeder, world, binding["id"], pod["id"])

        _publish(broker, run["id"])
        after = wait_for_run(db, run["id"], status=COMPLETED, timeout=RUN_TIMEOUT)
        assert after["status"] == COMPLETED, (
            f"run ended {after['status']}/{after['error_code']}: {after['error']}"
        )

        ledger = _create_ledger_rows(db, run["id"])
        assert [row["status"] for row in ledger] == ["succeeded"], (
            f"the create tool must run exactly once: {ledger!r}; "
            f"stub saw {stub.create_emitted} create emission(s) "
            f"in {len(stub.requests)} request(s): {stub.requests!r}"
        )

        issue = _issue_by_name(db, project["id"], title)
        assert issue is not None, "the tool call must create the issue"
        assert str(issue["state_id"]) == str(backlog["id"]), (
            "the bounce must move the issue to the Backlog state"
        )
        _track_side_effects(seeder, db, run["id"], issue["id"])

        tickers = _ticker_rows(db, issue["id"])
        assert len(tickers) == 1, "the bounce leaves exactly one (dormant) ticker"
        ticker = tickers[0]
        assert ticker["enabled"] is False
        assert ticker["disarm_reason"] == "left_ticking_state"
        assert ticker["next_run_at"] is None

        comments = _comment_rows(db, issue["id"])
        assert len(comments) == 1, "the bounce posts exactly one notice"
        assert "no eligible runner" in comments[0]["comment_html"]

        assert _entry_runs(db, issue["id"]) == []
    finally:
        stub.stop()
