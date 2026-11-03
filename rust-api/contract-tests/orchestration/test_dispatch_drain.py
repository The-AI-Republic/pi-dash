"""Orchestration drains the target pod after creating a run (PIDASHCONV-1494).

The jobs-layer creation drivers run ``dispatch_agent_run`` after commit and
return its ``DispatchDecision``s so the caller can drain the target pod.
Five live callers dropped those decisions, so a run created for a local
runner stayed QUEUED instead of being assigned straight away: the v1
``run-ai`` / ``re-tick`` handlers, the ``fire_tick`` continuation seam, the
scheduler dispatch, and the worker-side project-move handoff. Django drains
inline (``cloud_agent/dispatch.py:61-70``: ``drain_pod_by_id(run.pod_id)``
via ``on_commit``); the Rust callers kept only the run id.

Each test seeds a project with a pod and an online, idle, assignable local
runner (heartbeat-fresh + an active session row, so the Assign enqueue hits
the live stream instead of raising ``RunnerOfflineError``), exercises one
path, and asserts the new run lands ASSIGNED on that runner. The ``run-ai``
201 body renders stale-queued on both backends (Django answers from the
in-memory pre-drain run); the row pins the drain.

Task-driven cases (fire_tick, scheduler, sweep) publish in Celery wire
format like the task-oracle suites: ``PIDASH_CONTRACT_BACKEND=django``
(default) publishes to the Redis broker (a Django celery worker must be
running); ``=rust`` seeds ``rust_job_queue`` (``pidash-api worker`` must be
running against the same ``DATABASE_URL``).
"""

from __future__ import annotations

import os
import secrets
from datetime import datetime, timedelta, timezone

import pytest

from _harness import db, rust_queue, world
from _harness.auth import authed_client, forge_session_key, get_secret_key
from _harness.celery import Broker, get_broker_url
from _harness.db import get_database_url
from _harness.env import BASE_URL

pytestmark = pytest.mark.contract

BACKEND = os.environ.get("PIDASH_CONTRACT_BACKEND", "django")

FIRE = "pi_dash.bgtasks.agent_ticker.fire_tick"
FIRE_BINDING = "pi_dash.bgtasks.scheduler.fire_scheduler_binding"
FAIL_SWEEP = "runner.fail_runs_on_offline_runners"

MOVE_PATH = "/api/workspaces/{slug}/projects/{project}/work-items/{issue}/move/"


@pytest.fixture()
def broker():
    if BACKEND == "rust":
        yield None
        return
    broker = Broker(get_broker_url())
    yield broker
    broker.close()


def _exec(sql, params=()):
    db.execute(get_database_url(), sql, params)


def _fetchone(sql, params=()):
    return db.fetchone(get_database_url(), sql, params)


def _fresh_runner(owner_id, workspace_id, pod_id):
    """Online idle runner with a live session row, ready for the matcher."""
    runner_id = world.make_online_runner(owner_id, workspace_id, pod_id)
    _exec(
        "UPDATE runner SET last_heartbeat_at=now(), status='online' WHERE id=%s",
        (runner_id,),
    )
    _exec(
        "INSERT INTO runner_session (id, runner_id, protocol_version,"
        " created_at, last_seen_at, revoked_at, revoked_reason)"
        " VALUES (%s,%s,4,now(),now(),NULL,'')",
        (world.uid(), runner_id),
    )
    return runner_id


def _run_row(run_id):
    return _fetchone(
        "SELECT status, runner_id, owner_id, assigned_at FROM agent_run WHERE id=%s",
        (run_id,),
    )


def _assert_assigned(run_id, runner_id):
    row = _run_row(run_id)
    assert row["status"] == "assigned", row
    assert str(row["runner_id"]) == runner_id, row
    assert row["owner_id"] is not None and row["assigned_at"] is not None
    return row


def _fire(task, broker, args):
    """Publish one task invocation; block until the worker settles it."""
    if BACKEND == "rust":
        celery_id = rust_queue.publish(task, list(args), {})
        rust_queue.wait_for_settled(celery_id, what=f"{task} settled")
        return
    broker.publish(task, list(args))
    # No broker-side settle signal on the Django path: the tests poll
    # the row effects below.


def test_run_ai_assigns_to_idle_runner(api_client):
    """v1 run-ai assigns the new run in-request with an idle local runner."""
    w, client = api_client
    pod = world.make_pod(w.workspace_id, w.project_id, is_default=True)
    runner = _fresh_runner(w.user_id, w.workspace_id, pod)
    issue = world.make_issue(w, "runai", "In Progress")

    r = client.post_run_ai(w.workspace_slug, w.project_id, issue)
    assert r.status_code == 201, r.text
    body = r.json()
    assert body["status"] == "queued"  # stale pre-drain render, both backends
    assert body["executor"] == "local_runner"
    _assert_assigned(body["id"], runner)


def test_retick_assigns_to_idle_runner(api_client):
    """v1 re-tick assigns the new run in-request with an idle local runner."""
    w, client = api_client
    pod = world.make_pod(w.workspace_id, w.project_id, is_default=True)
    runner = _fresh_runner(w.user_id, w.workspace_id, pod)
    issue = world.make_issue(w, "retick", "In Progress")
    world.set_ticker(issue, used=10)  # spent pool: the grant dispatches now

    r = client.post_retick(w.workspace_slug, w.project_id, issue)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["granted"] is True, body
    assert body["run_id"] is not None, body
    _assert_assigned(body["run_id"], runner)


def test_fire_tick_continuation_assigns(api_client, broker):
    """A due tick with no active run creates a continuation already assigned."""
    w, _ = api_client
    pod = world.make_pod(w.workspace_id, w.project_id, is_default=True)
    runner = _fresh_runner(w.user_id, w.workspace_id, pod)
    issue = world.make_issue(w, "fire", "In Progress")
    world.make_run(issue, w.workspace_id, w.user_id, pod, status="completed")
    world.set_ticker(
        issue, used=1,
        next_run_at=datetime.now(timezone.utc) - timedelta(minutes=5),
    )
    ticker_id = str(_fetchone(
        "SELECT id FROM issue_agent_ticker WHERE issue_id=%s", (issue,),
    )["id"])

    _fire(FIRE, broker, [ticker_id])

    def _claimed():
        tick = _fetchone(
            "SELECT last_tick_at FROM issue_agent_ticker WHERE issue_id=%s", (issue,),
        )
        return tick and tick["last_tick_at"] is not None

    db.wait_for_condition(_claimed, what="fire_tick claimed")

    def _assigned():
        # The claim commits before the dispatch (test_ticker_tasks.py
        # documents the window): poll the converged assigned row, not
        # the claim mark. Pre-fix this times out (run stuck queued).
        row = _fetchone(
            "SELECT id, status FROM agent_run WHERE work_item_id=%s AND status IN"
            " ('queued','assigned') ORDER BY created_at DESC LIMIT 1",
            (issue,),
        )
        return row is not None and row["status"] == "assigned"

    db.wait_for_condition(_assigned, what="fire_tick continuation assigned")
    new_run = _fetchone(
        "SELECT id FROM agent_run WHERE work_item_id=%s AND status IN"
        " ('queued','assigned') ORDER BY created_at DESC LIMIT 1",
        (issue,),
    )
    assert new_run is not None, "fire_tick created no continuation run"
    _assert_assigned(new_run["id"], runner)


def test_scheduler_binding_fire_assigns(api_client, broker):
    """A scheduler binding fire assigns the new run to the pod's runner."""
    w, _ = api_client
    pod = world.make_pod(w.workspace_id, w.project_id, is_default=True)
    runner = _fresh_runner(w.user_id, w.workspace_id, pod)
    sched_id = world.uid()
    _exec(
        "INSERT INTO schedulers (id, slug, name, description, prompt, source,"
        " is_enabled, workspace_id, color, created_at, updated_at, created_by_id)"
        " VALUES (%s,%s,%s,'','Scan the repo.','builtin',true,%s,'#3b82f6',"
        "now(),now(),%s)",
        (sched_id, f"sch-{secrets.token_hex(4)}", "drain-probe", w.workspace_id,
         w.user_id),
    )
    binding_id = world.uid()
    past = (datetime.now(timezone.utc) - timedelta(hours=2)).isoformat()
    _exec(
        "INSERT INTO scheduler_bindings (id, extra_context, enabled, last_error,"
        " workspace_id, project_id, scheduler_id, dtstart, tzid, rrule, rdates,"
        " exdates, outcome_mode, created_at, updated_at, actor_id, pod_id,"
        " next_run_at)"
        " VALUES (%s,'',true,'',%s,%s,%s,%s,'UTC','FREQ=HOURLY','[]','[]',"
        "'create_issue',now(),now(),%s,%s,%s)",
        (binding_id, w.workspace_id, w.project_id, sched_id, past, w.user_id,
         pod, past),
    )

    _fire(FIRE_BINDING, broker, [binding_id])

    def _fired():
        binding = _fetchone(
            "SELECT last_run_id FROM scheduler_bindings WHERE id=%s", (binding_id,),
        )
        return binding and binding["last_run_id"] is not None

    db.wait_for_condition(_fired, what="scheduler binding fired")
    run_id = _fetchone(
        "SELECT last_run_id FROM scheduler_bindings WHERE id=%s", (binding_id,),
    )["last_run_id"]
    _assert_assigned(run_id, runner)


def test_sweep_handoff_assigns_replacement(db_conn, broker):
    """Offline sweep reaps a moved run; the replacement lands assigned.

    The worker-side move handoff (PIDASHCONV-1219 covered the API-side
    revoke variant): move an issue with an active local run to another
    project, let the source runner go silent, and fire
    ``runner.fail_runs_on_offline_runners``. The sweep finalizes the
    parked source run, the handoff creates the replacement in the target
    pod, and the target pod drains to its idle runner.
    """
    w = world.build("dd1494e")
    tgt_proj, tgt_ident = world.make_project(
        w.workspace_id, w.user_id, "T" + secrets.token_hex(2).upper()
    )
    world.add_project_member(tgt_proj, w.workspace_id, w.user_id)
    tgt_state = world.make_state(
        tgt_proj, w.workspace_id, "Backlog", "backlog", default=True, sequence=1.0
    )
    _exec(
        "UPDATE projects SET default_state_id=%s WHERE id=%s", (tgt_state, tgt_proj)
    )
    src_pod = world.make_pod(w.workspace_id, w.project_id, is_default=True)
    tgt_pod = world.make_pod(w.workspace_id, tgt_proj, is_default=True)
    issue = world.make_issue(w, "sweep", "Backlog")
    src_runner = world.make_online_runner(w.user_id, w.workspace_id, src_pod)
    tgt_runner = _fresh_runner(w.user_id, w.workspace_id, tgt_pod)
    src_run = world.make_run(
        issue, w.workspace_id, w.user_id, src_pod,
        status="running", runner_id=src_runner,
    )
    _exec(
        "UPDATE agent_run SET assigned_at=now() - interval '2 hours'"
        " WHERE id=%s",
        (src_run,),
    )
    password_field = _fetchone(
        "SELECT password FROM users WHERE id=%s", (w.user_id,)
    )["password"]
    key = forge_session_key(
        db_conn, user_id=w.user_id, password_field=password_field,
        secret=get_secret_key(),
    )
    db_conn.commit()
    client = authed_client(BASE_URL, key)
    try:
        moved = client.post(
            MOVE_PATH.format(
                slug=w.workspace_slug, project=w.project_id, issue=issue
            ),
            json={"project": tgt_ident},
        )
        assert moved.status_code == 200, moved.text
        parked = _fetchone(
            "SELECT status FROM agent_run WHERE id=%s", (src_run,)
        )
        assert parked["status"] == "cancel_requested", parked

        # The source runner goes silent; the sweep reaps the parked run.
        _exec(
            "UPDATE runner SET last_heartbeat_at=now() - interval '2 hours'"
            " WHERE id=%s",
            (src_runner,),
        )
        _fire(FAIL_SWEEP, broker, [])

        def _replaced():
            return _fetchone(
                "SELECT id FROM agent_run WHERE work_item_id=%s AND parent_run_id=%s",
                (issue, src_run),
            ) is not None

        db.wait_for_condition(_replaced, what="handoff replacement created")

        def _replacement_assigned():
            # The replacement row commits before its on_commit target
            # drain lands: poll the converged assigned row. Pre-fix
            # this times out (replacement stuck queued).
            row = _fetchone(
                "SELECT status FROM agent_run"
                " WHERE work_item_id=%s AND parent_run_id=%s",
                (issue, src_run),
            )
            return row is not None and row["status"] == "assigned"

        db.wait_for_condition(
            _replacement_assigned, what="handoff replacement assigned"
        )
        replacement = _fetchone(
            "SELECT id, pod_id FROM agent_run"
            " WHERE work_item_id=%s AND parent_run_id=%s",
            (issue, src_run),
        )
        assert str(replacement["pod_id"]) == tgt_pod, replacement
        _assert_assigned(replacement["id"], tgt_runner)
    finally:
        client.close()
