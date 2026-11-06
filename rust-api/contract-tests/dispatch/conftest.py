"""Shared fixtures for the dispatch task-oracle suite (PIDASHCONV-22, D-11).

Every test seeds its own world straight into Postgres (unique tag per test)
and deletes only its own rows afterwards. Jobs go to the broker in Celery
wire format (``_harness.celery.Broker``); the live Django worker executes
them. Nothing here imports Django.

``AgentRun`` rows are seeded with raw SQL. Column lists mirror
``apps/api/pi_dash/runner/models.py`` (``AgentRun`` → ``agent_run``,
``Pod`` → ``pod``); ``work_item``/``scheduler_binding`` stay NULL so the
run resolves its project through ``pod.project`` (``cloud_agent/tasks.py``
``run_cloud_agent`` falls back to ``run.pod.project``), which keeps the
seeded world to users + workspace + project + pod.
"""

from __future__ import annotations

import json
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

from _harness.celery import Broker, get_broker_url
from _harness.config import get_settings
from _harness.db import LazyDatabase
from _harness.seed import Seeder, SeedTracker

# Executor / status literals from pi_dash/core/agent_execution.py and
# pi_dash/runner/models.py AgentRunStatus.
CLOUD_AGENT = "cloud_agent"
MANAGED_RUNNER = "managed_runner"
LOCAL_RUNNER = "local_runner"
QUEUED = "queued"
RUNNING = "running"
FAILED = "failed"
COMPLETED = "completed"
CANCELLED = "cancelled"

#: How long to wait for the worker to produce a DB effect. Tasks are fast
#: (DB transitions, no LLM calls on the covered branches); the budget covers
#: a loaded CI worker and broker round trips.
WORKER_TIMEOUT_SECONDS = 30


@pytest.fixture(scope="session")
def settings():
    return get_settings()


@pytest.fixture(scope="session")
def broker_url():
    return get_broker_url()


@pytest.fixture()
def db(settings):
    database = LazyDatabase(settings.database_url)
    yield database
    database.close()


@pytest.fixture()
def broker(broker_url):
    broker = Broker(broker_url)
    yield broker
    broker.close()


@pytest.fixture()
def seeder(db):
    tracker = SeedTracker(db)
    seeder = Seeder(db, tracker, tag=uuid.uuid4().hex[:10])
    yield seeder
    tracker.cleanup()


def _membership_world(seeder, *, role=20):
    """One workspace + member owner + project + project membership + pod.

    The owner is a workspace member and a project ADMIN by default, so
    ``run_cloud_agent`` passes the actor gates and reaches the
    LLM-config check (seeded users have no LLM config).
    """
    seeder.ensure_instance()
    owner = seeder.create_user()
    workspace = seeder.create_workspace(owner["id"])
    seeder.create_workspace_member(workspace["id"], owner["id"], role=role)
    project = seeder.create_project(workspace["id"])
    seeder.create_project_member(workspace["id"], project["id"], owner["id"], role=20)
    pod = create_pod(seeder, workspace["id"], project["id"], owner["id"])
    return {"owner": owner, "workspace": workspace, "project": project, "pod": pod}


@pytest.fixture()
def world(seeder):
    return _membership_world(seeder)


def create_pod(seeder, workspace_id, project_id, owner_id, *, name="Contract pod"):
    pod_id = str(uuid.uuid4())
    seeder.db.execute(
        """INSERT INTO pod
           (id, name, description, is_default, deleted_at, created_at, updated_at,
            created_by_id, workspace_id, project_id)
           VALUES (%s,%s,'',false,NULL,now(),now(),%s,%s,%s)""",
        (pod_id, name, owner_id, workspace_id, project_id),
    )
    seeder.track.add("pod", pod_id)
    return {"id": pod_id}


def create_agent_run(
    seeder,
    workspace_id,
    pod_id,
    creator_id,
    *,
    executor_kind=CLOUD_AGENT,
    status=QUEUED,
    prompt="contract run",
    tool_plan="{}",
    created_ago_seconds=0,
    started_ago_seconds=None,
    lease_in_seconds=None,
    cancel_reason=None,
    work_item_id=None,
):
    """Seed one ``agent_run`` row. ``created_ago_seconds`` backdates
    ``created_at`` for expiry tests; ``started_ago_seconds`` backdates
    ``started_at`` for staleness tests; ``lease_in_seconds`` sets a future
    (positive) ``lease_expires_at``. Timestamps are computed in Python so
    every parameter has a concrete type on the wire."""
    from datetime import datetime, timedelta, timezone

    now = datetime.now(timezone.utc)
    run_id = str(uuid.uuid4())
    seeder.db.execute(
        """INSERT INTO agent_run
           (id, status, prompt, run_config, required_capabilities, thread_id,
            lease_expires_at, done_payload, error_details, created_at, assigned_at,
            started_at, ended_at, owner_id, workspace_id, runner_id, work_item_id,
            parent_run_id, pod_id, created_by_id, pinned_runner_id,
            scheduler_binding_id, llm_model,
            trigger, prompt_manifest, executor_kind, dispatch_attempts,
            cancel_requested_at, cancel_reason, tool_plan,
            terminal_hooks_applied_at, terminal_capacity_released_at,
            phase_kind, agent_metadata, usage)
           VALUES (%s,%s,%s,'{}','[]','',
                   %s,
                   NULL,'{}',%s,NULL,
                   %s,
                   NULL,NULL,%s,NULL,%s,NULL,%s,%s,NULL,NULL,'',
                   'direct',NULL,%s,0,
                   %s,
                   %s,%s,
                   NULL,NULL,'','{}','{}')""",
        (
            run_id, status, prompt,
            now + timedelta(seconds=lease_in_seconds) if lease_in_seconds is not None else None,
            now - timedelta(seconds=created_ago_seconds),
            now - timedelta(seconds=started_ago_seconds) if started_ago_seconds is not None else None,
            workspace_id, work_item_id, pod_id, creator_id,
            executor_kind,
            now if cancel_reason is not None else None,
            cancel_reason or "",
            tool_plan,
        ),
    )
    seeder.track.add("agent_run", run_id)
    return {"id": run_id}


def create_llm_config(seeder, user_id):
    """Seed a BYOK marker row so ``user_has_llm_config`` passes.

    ``has_api_key`` is just ``bool(api_key_encrypted)`` — the bytes are
    never decrypted on the branches this suite reaches (the prompt-size and
    cancel checks land before model resolution), so a dummy ciphertext
    proves the gate without any provider credential.
    """
    seeder.db.execute(
        """INSERT INTO assistant_user_llm_config
           (provider_kind, base_url, model_name, api_key_encrypted,
            last_verified_at, created_at, updated_at, user_id)
           VALUES ('openai_compatible','https://llm.example.invalid/v1','contract-model',
                   decode('deadbeef','hex'),NULL,now(),now(),%s)""",
        (user_id,),
    )
    row_id = seeder.db.fetchval(
        "SELECT id FROM assistant_user_llm_config WHERE user_id = %s", (user_id,)
    )
    seeder.track.add("assistant_user_llm_config", row_id)
    return {"id": row_id}


def get_run(db, run_id):
    """The ``agent_run`` row plus the model's read-only ``error_code`` /
    ``error`` properties, read off the ``error_details`` bag
    (PDASHOSS01-187) exactly as ``AgentRun`` exposes them."""
    row = db.fetchone("SELECT * FROM agent_run WHERE id = %s", (run_id,))
    if row is not None:
        bag = row.get("error_details") or {}
        row["error_code"] = bag.get("code") or ""
        row["error"] = bag.get("message") or ""
    return row


def run_events(db, run_id):
    return db.fetchall(
        "SELECT kind, payload FROM agent_run_event WHERE agent_run_id = %s ORDER BY seq",
        (run_id,),
    )


def wait_for_run(db, run_id, *, timeout=WORKER_TIMEOUT_SECONDS, poll=0.2, **conditions):
    """Poll one ``agent_run`` row until every ``column=value`` condition holds.

    Returns the row. Raises ``AssertionError`` on timeout with the last row
    seen, so failures show the actual DB state (the before/after diff).
    """
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        last = get_run(db, run_id)
        if last is not None and all(last.get(col) == val for col, val in conditions.items()):
            return last
        time.sleep(poll)
    raise AssertionError(
        f"timed out waiting for agent_run {run_id} {conditions}; last row: {last}"
    )


def wait_for_queue_drain(broker, *, queue="celery", baseline=0, timeout=WORKER_TIMEOUT_SECONDS, poll=0.2):
    """Wait until the worker has consumed everything published so far.

    ``baseline`` is the queue length before this test published (normally
    0); a consumed message may publish follow-ons, so quiescence needs two
    consecutive quiet reads.
    """
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if broker.queue_length(queue) <= baseline:
            # One more beat: a consumed message may publish follow-ons.
            time.sleep(0.5)
            if broker.queue_length(queue) <= baseline:
                return
        time.sleep(poll)
    raise AssertionError(f"timed out waiting for queue {queue!r} to drain to {baseline}")


# --- Multi-tenant validation harness (NEWPIDASH-54) -----------------------
#
# The tests in ``test_multi_tenant*.py`` validate the hand-rolled Rust
# runtime (and the Django oracle) with several tenants sharing one worker:
# separate workspaces/users/projects/pods, one OpenAI-compatible BYOK per
# tenant pointing at its own recording stub, plus tenants with no config
# and an Anthropic config. Everything below is additive: the single-tenant
# helpers above are untouched.

RUN_TASK = "cloud_agent.run_agent_run"
STUB_MODEL = "oracle-stub"

_channel = None


def _live_channel():
    """The live worker's delivery channel, detected once per process.

    A Django worker answers Celery inspect and consumes the redis wire;
    otherwise the Rust worker claims ``rust_job_queue`` rows. Exactly one
    worker runs in any contract run, so exactly one channel delivers.
    """
    global _channel
    from _harness import broker_probe, rust_queue

    if _channel is None:
        try:
            live = bool(broker_probe.registered_tasks(timeout=5.0))
        except Exception:
            live = False
        _channel = "celery" if live else "rust_queue"
        if _channel == "rust_queue":
            rust_queue.ensure_schema()
    return _channel


def publish_task(broker, task, args=()):
    """Publish an arbitrary task through the live worker's channel."""
    if _live_channel() == "celery":
        broker.publish(task, list(args))
    else:
        from _harness import rust_queue

        rust_queue.publish(task, list(args))


def publish_run(broker, run_id):
    """Publish one run through the live worker's channel (detected once)."""
    publish_task(broker, RUN_TASK, [run_id])


def create_member_world(seeder, *, role=20):
    """Public wrapper over the membership world (one tenant's base)."""
    return _membership_world(seeder, role=role)


def seed_llm_config(
    seeder,
    user_id,
    base_url,
    api_key,
    *,
    provider_kind="openai_compatible",
    model_name=STUB_MODEL,
):
    """Seed a decryptable BYOK config pointing at a stub endpoint.

    The Fernet token is encrypted with the backend's
    ``ASSISTANT_ENCRYPTION_KEY`` so both Django and Rust decrypt the same
    ciphertext. For the ``anthropic`` kind the base URL is pinned to the
    provider default on both stacks (``llm.py:103``) and ignored — those
    configs are used for parity/non-contamination cases, never for
    recording provider traffic.
    """
    import os

    from cryptography.fernet import Fernet

    key = os.environ["ASSISTANT_ENCRYPTION_KEY"].encode("utf-8")
    token = Fernet(key).encrypt(api_key.encode("utf-8"))
    seeder.db.execute(
        """INSERT INTO assistant_user_llm_config
           (provider_kind, base_url, model_name, api_key_encrypted,
            last_verified_at, created_at, updated_at, user_id)
           VALUES (%s,%s,%s,%s,NULL,now(),now(),%s)""",
        (provider_kind, base_url, model_name, token, user_id),
    )
    row_id = seeder.db.fetchval(
        """SELECT id FROM assistant_user_llm_config WHERE user_id = %s
           ORDER BY created_at DESC LIMIT 1""",
        (user_id,),
    )
    seeder.track.add("assistant_user_llm_config", row_id)
    return {"id": row_id}


def rotate_llm_key(seeder, config_id, new_api_key):
    """Replace a config's key (new ciphertext) — the cache must miss."""
    import os

    from cryptography.fernet import Fernet

    key = os.environ["ASSISTANT_ENCRYPTION_KEY"].encode("utf-8")
    token = Fernet(key).encrypt(new_api_key.encode("utf-8"))
    seeder.db.execute(
        "UPDATE assistant_user_llm_config SET api_key_encrypted = %s WHERE id = %s",
        (token, config_id),
    )


def delete_llm_config(seeder, config_id):
    """Delete a config row: the user falls back to ``llm_config_missing``."""
    seeder.db.execute(
        "DELETE FROM assistant_user_llm_config WHERE id = %s", (config_id,)
    )


def tool_calls(db, run_id):
    """The run's ``agent_run_tool_call`` ledger rows, oldest first."""
    return db.fetchall(
        """SELECT tool_name, risk, status, error_code, safe_replay_result
           FROM agent_run_tool_call WHERE agent_run_id = %s ORDER BY prepared_at""",
        (run_id,),
    )


def wait_for_terminal(db, run_id, *, timeout=WORKER_TIMEOUT_SECONDS, poll=0.2):
    """Poll one ``agent_run`` row until it reaches a terminal status.

    Terminal states here are COMPLETED / FAILED (CANCELLED / REFUSED
    are terminal too, and this helper accepts any of them); returns the
    row via :func:`get_run`.
    """
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        row = db.fetchone("SELECT status FROM agent_run WHERE id = %s", (run_id,))
        if row is not None and row["status"] in (COMPLETED, FAILED, CANCELLED, "refused"):
            return get_run(db, run_id)
        last = row
        time.sleep(poll)
    raise AssertionError(
        f"timed out waiting for terminal state on run {run_id}; last: {last}"
    )


def _completion(message, *, finish_reason="stop", model_name=STUB_MODEL):
    return {
        "id": f"stub-{uuid.uuid4().hex[:8]}",
        "object": "chat.completion",
        "created": 1700000000,
        "model": model_name,
        "choices": [
            {"index": 0, "message": message, "finish_reason": finish_reason}
        ],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15},
    }


def final_response(summary="multi-tenant stub done", *, outcome="completed"):
    """One chat-completion carrying a valid ``CloudAgentOutput`` JSON."""
    return _completion(
        {"role": "assistant", "content": json.dumps({"outcome": outcome, "summary": summary})}
    )


def tool_call_response(call_id, name, arguments, *, reasoning=None):
    """One chat-completion carrying a single tool call."""
    message = {
        "role": "assistant",
        "content": None,
        "tool_calls": [
            {
                "id": call_id,
                "type": "function",
                "function": {"name": name, "arguments": json.dumps(arguments)},
            }
        ],
    }
    if reasoning is not None:
        message["reasoning_content"] = reasoning
    return _completion(message, finish_reason="tool_calls")


class ScriptedProvider:
    """Recording OpenAI-compatible stub serving one scripted response per request.

    ``script`` is a list of response payloads (dicts as returned by
    :func:`final_response` / :func:`tool_call_response`); request ``n``
    gets entry ``n`` (the last entry repeats so retry loops observe a
    stable provider). Every request is recorded with its headers (the
    ``Authorization`` header carries the per-tenant key proof) and body.
    """

    def __init__(self, script):
        self.script = list(script)
        self.requests: list[dict] = []
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
                try:
                    payload = json.loads(raw or b"{}")
                except ValueError:
                    payload = {}
                stub.requests.append(
                    {
                        "path": self.path,
                        # Header names are case-insensitive on the wire and
                        # the two stacks spell them differently; normalize
                        # to lowercase for lookup.
                        "headers": {
                            key.lower(): value
                            for key, value in self.headers.items()
                        },
                        "body": payload,
                    }
                )
                index = min(len(stub.requests) - 1, len(stub.script) - 1)
                content = json.dumps(stub.script[index]).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(content)))
                self.end_headers()
                self.wfile.write(content)

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self._server.daemon_threads = True
        port = self._server.server_address[1]
        self._thread = threading.Thread(
            target=self._server.serve_forever, daemon=True
        )
        self._thread.start()
        return f"http://127.0.0.1:{port}/v1"

    def stop(self):
        if self._server is not None:
            self._server.shutdown()
            self._server.server_close()
            self._server = None

    def authorization_headers(self):
        """The recorded ``Authorization`` values, in request order."""
        return [req["headers"].get("authorization") for req in self.requests]

    def assert_key_never_appears(self, key_fragment):
        """No recorded request may carry this key fragment anywhere."""
        for req in self.requests:
            haystack = json.dumps(req["headers"]) + json.dumps(req["body"])
            assert key_fragment not in haystack, (
                f"key {key_fragment!r} leaked to this stub: {req}"
            )
