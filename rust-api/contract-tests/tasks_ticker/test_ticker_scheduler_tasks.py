"""D-10 oracle: agent ticker + scheduler + loop workers and the beat schedule.

Django sources: pi_dash/bgtasks/{agent_ticker, scheduler, loop, _rrule}.py
plus the beat schedule in pi_dash/celery.py.

Safety: scan tests seed due rows, publish the scan, then drain the fanned-out
fire_* messages from the broker WITHOUT executing them — firing a real tick
would dispatch live agent runs. The drain itself is the fan-out proof (task
name + row id on the wire). ``fire_tick`` with an unknown id returns False;
the suite asserts it is consumed without dispatching anything.
"""

import uuid
from datetime import datetime, timedelta, timezone

import pytest

from _harness import broker_probe, celery_wire, taskspec
from _harness import seed as seed_helpers
from _harness.db import insert_row, wait_for

M = "pi_dash.bgtasks"

TICKER_TASKS = {
    f"{M}.agent_ticker.scan_due_tickers": ("pi_dash/bgtasks/agent_ticker.py", "scan_due_tickers"),
    f"{M}.agent_ticker.fire_tick": ("pi_dash/bgtasks/agent_ticker.py", "fire_tick"),
    f"{M}.scheduler.scan_due_bindings": ("pi_dash/bgtasks/scheduler.py", "scan_due_bindings"),
    f"{M}.scheduler.fire_scheduler_binding": (
        "pi_dash/bgtasks/scheduler.py",
        "fire_scheduler_binding",
    ),
    f"{M}.loop.scan_due_targets": ("pi_dash/bgtasks/loop.py", "scan_due_targets"),
    f"{M}.loop.fire_loop_target": ("pi_dash/bgtasks/loop.py", "fire_loop_target"),
}


@pytest.mark.parametrize("task_name", sorted(TICKER_TASKS))
def test_wire_payload_parity(task_name):
    message = celery_wire.capture_wire_message(task_name, args=["probe"], kwargs={})
    celery_wire.assert_wire_message(message, task_name, args=["probe"], kwargs={})


def test_task_options_parity():
    """Ack/retry parity: scans are plain; fires bind with max_retries=0."""
    taskspec.assert_task_options(
        "pi_dash/bgtasks/agent_ticker.py", "scan_due_tickers", {}
    )
    taskspec.assert_task_options("pi_dash/bgtasks/agent_ticker.py", "fire_tick", {})
    taskspec.assert_task_options(
        "pi_dash/bgtasks/scheduler.py", "scan_due_bindings", {}
    )
    taskspec.assert_task_options(
        "pi_dash/bgtasks/scheduler.py",
        "fire_scheduler_binding",
        {"bind": "True", "max_retries": "0"},
    )
    taskspec.assert_task_options("pi_dash/bgtasks/loop.py", "scan_due_targets", {})
    taskspec.assert_task_options(
        "pi_dash/bgtasks/loop.py",
        "fire_loop_target",
        {"bind": "True", "max_retries": "0"},
    )
    # No task in this group overrides the ack behaviour.
    for module, func in [
        ("pi_dash/bgtasks/agent_ticker.py", "scan_due_tickers"),
        ("pi_dash/bgtasks/agent_ticker.py", "fire_tick"),
        ("pi_dash/bgtasks/scheduler.py", "scan_due_bindings"),
        ("pi_dash/bgtasks/scheduler.py", "fire_scheduler_binding"),
        ("pi_dash/bgtasks/loop.py", "scan_due_targets"),
        ("pi_dash/bgtasks/loop.py", "fire_loop_target"),
    ]:
        taskspec.assert_no_option(module, func, "acks_late")


def test_fan_out_call_sites():
    taskspec.assert_calls_delay(
        "pi_dash/bgtasks/agent_ticker.py", "scan_due_tickers", "fire_tick"
    )
    taskspec.assert_calls_delay(
        "pi_dash/bgtasks/scheduler.py", "scan_due_bindings", "fire_scheduler_binding"
    )


def test_beat_entry_parity():
    owned = {
        "scan-due-agent-tickers": (
            f"{M}.agent_ticker.scan_due_tickers",
            'crontab(minute="*")',
        ),
        "scan-due-scheduler-bindings": (
            f"{M}.scheduler.scan_due_bindings",
            'crontab(minute="*")',
        ),
        "scan-due-loop-targets": (
            f"{M}.loop.scan_due_targets",
            'crontab(minute="*")',
        ),
        "github-issue-sync-every-4h": (
            "pi_dash.bgtasks.git_sync_task.sync_all_bindings",
            'crontab(minute=0, hour="*/4")',
        ),
    }
    for entry, (task, schedule) in owned.items():
        taskspec.assert_beat_entry(entry, task, schedule)


def test_worker_registration(broker_url):
    broker_probe.wait_for_registration(set(TICKER_TASKS))


def _past(minutes: int = 5) -> str:
    return (datetime.now(timezone.utc) - timedelta(minutes=minutes)).isoformat()


def _seed_issue_chain(db_conn, slug: str):
    """user → workspace → project → state → issue. Returns dict of rows."""
    owner = seed_helpers.user(db_conn, f"contract-{slug}-owner")
    workspace = seed_helpers.workspace(db_conn, f"contract{slug}", owner["id"])
    project = insert_row(
        db_conn,
        "projects",
        {
            "workspace_id": str(workspace["id"]),
            "name": f"contract {slug}",
            "description": "",
            "identifier": "CT",
            "network": 2,
            "agent_default_max_ticks": 10,
        },
    )
    state = insert_row(
        db_conn,
        "states",
        {
            "project_id": str(project["id"]),
            "workspace_id": str(workspace["id"]),
            "name": "Contract",
            "description": "",
            "color": "#000000",
            "group": "backlog",
            "sequence": 65535,
        },
    )
    issue = insert_row(
        db_conn,
        "issues",
        {
            "workspace_id": str(workspace["id"]),
            "project_id": str(project["id"]),
            "state_id": str(state["id"]),
            "name": "contract issue",
            "description_json": {},
            "description_html": "<p></p>",
            "priority": "none",
            "is_draft": False,
            "sort_order": 65535,
            "workpad": "",
        },
    )
    return {"owner": owner, "workspace": workspace, "project": project,
            "state": state, "issue": issue}


def _quiesce_tickers(conn, keep_id=None) -> None:
    """Disable every ticker except ``keep_id`` — scans are global, tests aren't."""
    with conn.cursor() as cur:
        if keep_id is None:
            cur.execute("UPDATE issue_agent_ticker SET enabled = FALSE")
        else:
            cur.execute(
                "UPDATE issue_agent_ticker SET enabled = FALSE WHERE id <> %s::uuid",
                (str(keep_id),),
            )
    conn.commit()


def _quiesce_bindings(conn, keep_id=None) -> None:
    with conn.cursor() as cur:
        if keep_id is None:
            cur.execute("UPDATE scheduler_bindings SET enabled = FALSE")
        else:
            cur.execute(
                "UPDATE scheduler_bindings SET enabled = FALSE WHERE id <> %s::uuid",
                (str(keep_id),),
            )
    conn.commit()


def test_scan_due_tickers_fans_out_fire_tick(db_conn, broker_url):
    """Beat-firing parity: a due ticker row becomes one fire_tick message."""
    chain = _seed_issue_chain(db_conn, "ticker")
    ticker = insert_row(
        db_conn,
        "issue_agent_ticker",
        {
            "issue_id": str(chain["issue"]["id"]),
            "used": 0,
            "granted": 0,
            "waited": 0,
            "enabled": True,
            "next_run_at": _past(),
        },
    )
    _quiesce_tickers(db_conn, ticker["id"])
    celery_wire.publish(f"{M}.agent_ticker.scan_due_tickers")
    fanned = broker_probe.collect_matching(
        lambda headers, _payload: headers.get("task")
        == f"{M}.agent_ticker.fire_tick"
    )
    assert len(fanned) == 1
    _headers, payload = fanned[0]
    assert list(payload[0]) == [str(ticker["id"])]


def test_scan_due_tickers_skips_not_due(db_conn, broker_url):
    """A ticker with a future next_run_at fans out nothing."""
    chain = _seed_issue_chain(db_conn, "notdue")
    future = (datetime.now(timezone.utc) + timedelta(hours=1)).isoformat()
    fresh = insert_row(
        db_conn,
        "issue_agent_ticker",
        {
            "issue_id": str(chain["issue"]["id"]),
            "used": 0,
            "granted": 0,
            "waited": 0,
            "enabled": True,
            "next_run_at": future,
        },
    )
    _quiesce_tickers(db_conn, fresh["id"])
    celery_wire.publish(f"{M}.agent_ticker.scan_due_tickers")
    broker_probe.wait_for_queue_drain(what="not-due scan consumed")
    leftovers = [
        (headers, _payload)
        for headers, _payload in broker_probe.drain_queue()
        if headers.get("task") == f"{M}.agent_ticker.fire_tick"
    ]
    assert leftovers == []


def test_fire_tick_unknown_id_dispatches_nothing(db_conn, broker_url):
    """fire_tick on a missing row returns False: consumed, no fan-out."""
    _quiesce_tickers(db_conn)
    _quiesce_bindings(db_conn)
    celery_wire.publish(
        f"{M}.agent_ticker.fire_tick", args=[str(uuid.uuid4())]
    )
    broker_probe.wait_for_queue_drain(what="fire_tick consumed")
    # Beat also fires scans into this queue; only fire_* dispatches count.
    stray = [
        (headers, _payload)
        for headers, _payload in broker_probe.drain_queue()
        if (headers.get("task") or "").endswith(("fire_tick", "fire_scheduler_binding", "fire_loop_target"))
    ]
    assert stray == []


def test_scan_due_bindings_fans_out_fire(db_conn, broker_url):
    """A due scheduler binding becomes one fire_scheduler_binding message."""
    chain = _seed_issue_chain(db_conn, "sched")
    scheduler = insert_row(
        db_conn,
        "schedulers",
        {
            "workspace_id": str(chain["workspace"]["id"]),
            "slug": f"contract-{uuid.uuid4().hex[:8]}",
            "name": "contract scheduler",
            "description": "",
            "prompt": "contract prompt",
            "source": "builtin",
            "is_enabled": True,
            "color": "#3b82f6",
        },
    )
    binding = insert_row(
        db_conn,
        "scheduler_bindings",
        {
            "workspace_id": str(chain["workspace"]["id"]),
            "project_id": str(chain["project"]["id"]),
            "scheduler_id": str(scheduler["id"]),
            "dtstart": _past(),
            "tzid": "UTC",
            "rrule": "",
            "rdates": [],
            "exdates": [],
            "extra_context": "",
            "enabled": True,
            "outcome_mode": "create_issue",
            "next_run_at": None,
        },
    )
    _quiesce_bindings(db_conn, binding["id"])
    celery_wire.publish(f"{M}.scheduler.scan_due_bindings")
    fanned = broker_probe.collect_matching(
        lambda headers, _payload: headers.get("task")
        == f"{M}.scheduler.fire_scheduler_binding"
    )
    assert len(fanned) == 1
    _headers, payload = fanned[0]
    assert list(payload[0]) == [str(binding["id"])]


def test_loop_scan_consumed_without_crash(db_conn, broker_url):
    """Loop eligibility needs deep runner state: assert consumed + acked."""
    baseline = broker_probe.queue_depth()
    celery_wire.publish(f"{M}.loop.scan_due_targets")
    broker_probe.wait_for_queue_drain(
        baseline=baseline, what="loop scan consumed"
    )


def _drain_fire_ticks():
    return [
        (headers, payload)
        for headers, payload in broker_probe.drain_queue()
        if headers.get("task") == f"{M}.agent_ticker.fire_tick"
    ]


def test_redelivered_scan_fans_out_once_per_run(db_conn, broker_url):
    """Redelivery: two scans of one due ticker → one fire message per scan."""
    chain = _seed_issue_chain(db_conn, "redel")
    ticker = insert_row(
        db_conn,
        "issue_agent_ticker",
        {
            "issue_id": str(chain["issue"]["id"]),
            "used": 0,
            "granted": 0,
            "waited": 0,
            "pending_entry": True,
            "enabled": True,
            "next_run_at": _past(),
        },
    )
    _quiesce_tickers(db_conn, ticker["id"])
    broker_probe.drain_queue()
    celery_wire.publish(f"{M}.agent_ticker.scan_due_tickers")
    first = wait_for(lambda: _drain_fire_ticks() or None, what="first scan fan-out")
    assert [list(payload[0]) for _, payload in first] == [[str(ticker["id"])]]
    celery_wire.publish(f"{M}.agent_ticker.scan_due_tickers")
    second = wait_for(lambda: _drain_fire_ticks() or None, what="second scan fan-out")
    assert [list(payload[0]) for _, payload in second] == [[str(ticker["id"])]]
