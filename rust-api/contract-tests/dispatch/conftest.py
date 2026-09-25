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

import time
import uuid

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
            lease_expires_at, done_payload, error, created_at, assigned_at,
            started_at, ended_at, owner_id, workspace_id, runner_id, work_item_id,
            parent_run_id, pod_id, created_by_id, pinned_runner_id,
            scheduler_binding_id, llm_model, refusal_category, queue_position,
            trigger, prompt_manifest, executor_kind, dispatch_attempts,
            cancel_requested_at, cancel_reason, error_code, tool_plan,
            terminal_hooks_applied_at, terminal_capacity_released_at,
            phase_kind, agent_metadata, usage)
           VALUES (%s,%s,%s,'{}','[]','',
                   %s,
                   NULL,'',%s,NULL,
                   %s,
                   NULL,NULL,%s,NULL,NULL,NULL,%s,%s,NULL,NULL,'','',
                   NULL,'direct',NULL,%s,0,
                   %s,
                   %s,'',%s,
                   NULL,NULL,'','{}','{}')""",
        (
            run_id, status, prompt,
            now + timedelta(seconds=lease_in_seconds) if lease_in_seconds is not None else None,
            now - timedelta(seconds=created_ago_seconds),
            now - timedelta(seconds=started_ago_seconds) if started_ago_seconds is not None else None,
            workspace_id, pod_id, creator_id,
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
    return db.fetchone("SELECT * FROM agent_run WHERE id = %s", (run_id,))


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
