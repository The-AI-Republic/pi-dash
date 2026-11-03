# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Oracle: cloud-agent transition fires the completion comment (PIDASHCONV-1492).

Django fires ``post_completion_comment`` from the ``github_signals``
post_save receiver (``bgtasks/github_signals.py:44-75``) on every Issue
save that transitions into a ``completed``-group state. The cloud-agent
tool ``pidash_transition_current_issue`` saves through ``Issue.save()``
(``cloud_agent/tools.py:333-335``), so the signal fires and a synced
issue closed by an agent gets its upstream completion comment.

Live oracle: seed an issue in a non-completed state with a
``GitIssueSync`` row whose metadata carries no ``completion_comment_id``,
run a cloud-agent run whose model emits a single
``pidash_transition_current_issue`` call moving it to a Done-group
state, and prove the comment task ran: the worker executes it with the
seeded invalid token, the provider POST fails, and the sync metadata
records ``completion_comment_error`` (``git_sync_task.py:304-318`` —
every in-try fault records, including auth and network failures, so the
assertion holds with or without provider network access).

Exactly-once shape: the transition tool is single-use per run
(``policy.py`` ``REPEATABLE_WRITE_TOOLS`` excludes it), the test asserts
exactly one succeeded ledger row for it, and the fix enqueues at most
one completion job per state-changing transition — so one recorded
comment attempt means exactly one job. A live worker consumes the queue
on both backends (Redis broker vs ``rust_job_queue``), so the enqueue
count itself is not observable backend-agnostically; the ledger plus
the effect is the contract. The negative case proves silence: with
``completion_comment_id`` already set, the run completes, the issue
moves, and the sync metadata is untouched.

Needs a worker on both backends (the run and the comment task both
execute there). Backend selection follows
``runner/test_offline_runner_sweeps.py``: ``PIDASH_CONTRACT_BACKEND``
``=django`` (default) publishes Celery wire v2 to the Redis broker (a
Django celery worker must be running); ``=rust`` seeds
``rust_job_queue`` (``pidash-api worker`` must be running against the
same ``DATABASE_URL``). No HTTP server is needed: the LLM config row is
seeded directly with a Fernet-encrypted key
(``tasks_assistant/test_rust_replay.py`` pattern), so the workers need
``CLOUD_AGENT_ENABLED=true``, ``CLOUD_AGENT_WRITES_ENABLED=true``,
``CLOUD_AGENT_BLOCK_PRIVATE_URLS=false`` (the stub is loopback),
``ASSISTANT_CRYPTO_BACKEND=fernet`` and the matching
``ASSISTANT_ENCRYPTION_KEY``.
"""

from __future__ import annotations

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
from integrations import seed as int_seed

from .conftest import (
    COMPLETED,
    create_agent_run,
    wait_for_queue_drain,
    wait_for_run,
)

pytestmark = pytest.mark.contract

BACKEND = os.environ.get("PIDASH_CONTRACT_BACKEND", "django")

RUN_TASK = "cloud_agent.run_agent_run"

TRANSITION_TOOL = "pidash_transition_current_issue"

#: Realistic direct-run tool plan for a work-item-bound run
#: (``policy.build_tool_plan(run_kind="direct", has_issue=True)`` with
#: writes enabled, minus the scheduler-only create tool and the github
#: tools, which need a verified GitHub App installation).
PLAN_TOOLS = sorted(
    [
        "pidash_get_current_issue",
        "pidash_list_current_issue_comments",
        "pidash_list_project_states",
        "pidash_search_project_issues",
        "pidash_list_linked_code_reviews",
        "pidash_list_issue_relations",
        "pidash_add_current_issue_comment",
        "pidash_update_current_issue_workpad",
        "pidash_transition_current_issue",
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


def _database_url() -> str:
    return os.environ["DATABASE_URL"]


class _TransitionStub:
    """OpenAI-compatible stub whose first completion is one transition call.

    Serves ``POST /v1/chat/completions`` in streaming and non-streaming
    form. Until a ``role: tool`` message appears in the request history
    (both the Django pydantic-ai lane and the Rust model-runtime lane
    send tool results back as OpenAI ``tool``-role messages), the stub
    answers with a single ``pidash_transition_current_issue`` call;
    afterwards it calls the ``final_result`` output tool so the run ends.
    """

    def __init__(self, state_id: str):
        self.transition_arguments = json.dumps({"state_id": state_id})
        self.final_arguments = json.dumps(FINAL_RESULT_ARGS)
        self.requests: list[dict] = []
        self.transition_emitted = 0
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
                    if not already_ran:
                        stub.transition_emitted += 1
                if already_ran:
                    return "call_final_1", "final_result", stub.final_arguments, "stop"
                return (
                    "call_transition_1",
                    TRANSITION_TOOL,
                    stub.transition_arguments,
                    "tool_calls",
                )

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


def _seed_run(seeder, world, issue_id: str) -> dict:
    """One QUEUED cloud-agent run bound to the issue with a direct-run plan."""
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
        world["pod"]["id"],
        world["owner"]["id"],
        prompt="Please mark the issue as done.",
        tool_plan=json.dumps(plan),
    )
    seeder.db.execute(
        "UPDATE agent_run SET work_item_id = %s WHERE id = %s",
        (issue_id, run["id"]),
    )
    return run


def _seed_sync_world(unique: str, ws_id: str, project_id: str, owner_id: str,
                     issue_id: str, metadata: dict):
    """Provider account + repo + binding + one GitIssueSync row. Returns
    (scope, sync_id); the caller must ``scope.cleanup()`` in a finally."""
    database_url = _database_url()
    anchor = {
        "workspace": {"id": str(ws_id)},
        "projects": [{"id": str(project_id), "workspace_id": str(ws_id)}],
        "actor": {"id": str(owner_id)},
    }
    scope = int_seed.Scope(database_url)
    account_id = int_seed.provider_account(database_url, anchor, scope)
    repo_id = int_seed.repository(database_url, scope, suffix=f"ct1492-{unique}")
    binding_id = int_seed.binding(
        database_url, anchor, scope, account_id=account_id, repository_id=repo_id
    )
    sync_id = int_seed.git_issue_sync(
        database_url,
        anchor,
        scope,
        binding_id=binding_id,
        issue_id=issue_id,
        external_iid=f"ct-1492-{unique}",
        metadata=dict(metadata),
    )
    return scope, sync_id


def _publish(broker, run_id: str):
    """Publish one run invocation; returns the queue baseline for drains."""
    if BACKEND == "rust":
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


def _sync_metadata(db, sync_id: str):
    row = db.fetchone("SELECT metadata FROM git_issue_syncs WHERE id = %s", (sync_id,))
    return row["metadata"] if row else None


def _issue_state(db, issue_id: str):
    return db.fetchone("SELECT state_id FROM issues WHERE id = %s", (issue_id,))["state_id"]


def _transition_ledger_rows(db, run_id: str):
    return db.fetchall(
        "SELECT status FROM agent_run_tool_call "
        "WHERE agent_run_id = %s AND tool_name = %s",
        (run_id, TRANSITION_TOOL),
    )


def test_transition_to_completed_enqueues_post_completion_comment(db, broker, seeder, world):
    """A run moving a synced issue to Done fires the completion comment.

    Seeds an issue in a started state mirrored by a ``GitIssueSync``
    row with empty metadata, drives one run whose model emits
    ``pidash_transition_current_issue(state_id=<Done>)``, then proves
    the comment task executed: the sync metadata records the provider
    failure (the seeded token is invalid, so success is impossible).
    Without the enqueue the metadata stays empty and the test fails.
    """
    owner, ws, project = world["owner"], world["workspace"], world["project"]
    started = seeder.create_state(
        ws["id"], project["id"], name="Doing", group="started"
    )
    done = seeder.create_state(ws["id"], project["id"], name="Done", group="completed")
    issue = seeder.create_issue(ws["id"], project["id"], started["id"])
    issue_id = issue["id"]

    unique = uuid.uuid4().hex[:8]
    scope, sync_id = _seed_sync_world(
        unique, ws["id"], project["id"], owner["id"], issue_id, {}
    )
    stub = _TransitionStub(str(done["id"]))
    base = stub.start()
    try:
        _seed_llm_config(seeder, owner["id"], base)
        run = _seed_run(seeder, world, issue_id)

        _publish(broker, run["id"])
        after = wait_for_run(db, run["id"], status=COMPLETED, timeout=RUN_TIMEOUT)
        assert after["status"] == COMPLETED, (
            f"run ended {after['status']}/{after['error_code']}: {after['error']}"
        )

        assert str(_issue_state(db, issue_id)) == str(done["id"]), (
            "the run's transition call must move the issue to Done"
        )
        ledger = _transition_ledger_rows(db, run["id"])
        assert [row["status"] for row in ledger] == ["succeeded"], (
            f"the transition tool must run exactly once: {ledger!r}; "
            f"stub saw {stub.transition_emitted} transition emission(s) "
            f"in {len(stub.requests)} request(s): {stub.requests!r}"
        )
        assert stub.transition_emitted == 1, (
            f"the model must emit the transition call exactly once: {stub.requests!r}"
        )

        def _comment_attempted():
            metadata = _sync_metadata(db, sync_id) or {}
            if metadata.get("completion_comment_id") or metadata.get(
                "completion_comment_error"
            ):
                return metadata
            return None

        metadata = wait_for(
            _comment_attempted,
            timeout=EFFECT_TIMEOUT,
            what="post_completion_comment executed",
        )
        assert metadata.get("completion_comment_id") or metadata.get(
            "completion_comment_error"
        ), f"sync metadata never recorded the comment attempt: {metadata!r}"
    finally:
        stub.stop()
        scope.cleanup()


def test_transition_with_existing_completion_comment_enqueues_nothing(
    db, broker, seeder, world
):
    """An already-commented sync row stays silent across the transition.

    Same drive as above, but the ``GitIssueSync`` metadata already
    carries ``completion_comment_id``: the run still completes and the
    issue still moves, yet no completion job may run — the metadata
    must be byte-identical afterwards (in particular, no
    ``completion_comment_error`` appears).
    """
    owner, ws, project = world["owner"], world["workspace"], world["project"]
    started = seeder.create_state(
        ws["id"], project["id"], name="Doing", group="started"
    )
    done = seeder.create_state(ws["id"], project["id"], name="Done", group="completed")
    issue = seeder.create_issue(ws["id"], project["id"], started["id"])
    issue_id = issue["id"]

    seeded_metadata = {"completion_comment_id": "ct-seeded-1"}
    unique = uuid.uuid4().hex[:8]
    scope, sync_id = _seed_sync_world(
        unique, ws["id"], project["id"], owner["id"], issue_id, seeded_metadata
    )
    stub = _TransitionStub(str(done["id"]))
    base = stub.start()
    try:
        _seed_llm_config(seeder, owner["id"], base)
        run = _seed_run(seeder, world, issue_id)

        baseline = _publish(broker, run["id"])
        after = wait_for_run(db, run["id"], status=COMPLETED, timeout=RUN_TIMEOUT)
        assert after["status"] == COMPLETED, (
            f"run ended {after['status']}/{after['error_code']}: {after['error']}"
        )
        assert str(_issue_state(db, issue_id)) == str(done["id"]), (
            "the run's transition call must move the issue to Done"
        )
        # Quiescence: an erroneous completion job would already be
        # queued (it enqueues during the transition tool call), so a
        # drained queue plus a settle sleep proves silence.
        _drain(broker, baseline)
        time.sleep(QUIESCE_SECONDS)
        assert _sync_metadata(db, sync_id) == seeded_metadata, (
            f"already-commented sync row must be untouched: "
            f"{_sync_metadata(db, sync_id)!r}"
        )
    finally:
        stub.stop()
        scope.cleanup()
