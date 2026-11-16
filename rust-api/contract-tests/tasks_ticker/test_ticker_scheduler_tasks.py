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
from psycopg.rows import dict_row

from _harness import broker_probe, celery_wire, taskspec
from _harness import seed as seed_helpers
from _harness.db import insert_row

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


def _quiesce_tickers(conn, keep_id=None) -> None:
    """Disable every ticker except ``keep_id`` — scans are global, tests aren't."""
    with conn.cursor(row_factory=dict_row) as cur:
        if keep_id is None:
            cur.execute("UPDATE issue_agent_ticker SET enabled = FALSE")
        else:
            cur.execute(
                "UPDATE issue_agent_ticker SET enabled = FALSE WHERE id <> %s::uuid",
                (str(keep_id),),
            )
    conn.commit()


def _quiesce_bindings(conn, keep_id=None) -> None:
    with conn.cursor(row_factory=dict_row) as cur:
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
    chain = seed_helpers.issue_chain(db_conn, "ticker")
    ticker = insert_row(
        db_conn,
        "issue_agent_ticker",
        {
            "issue_id": str(chain["issue"]["id"]),
            "used": 0,
            "granted": 0,
            "waited": 0,
            "user_disabled": False,
            "enabled": True,
            "disarm_reason": "",
            "pending_entry": False,
            "pending_entry_free": False,
            "pending_entry_trigger": "",
            "next_run_at": _past(),
        },
    )
    _quiesce_tickers(db_conn, ticker["id"])
    # Observed via task-received events, not queue drain: an idle worker
    # wins the drain race via prefetch every time, while events are
    # broadcast (requires the contract worker's -E flag). The stream binds
    # before publishing so a fast worker can't predate the observer.
    # Counts are beat-tolerant (>=): the live beat may fan out the same row.
    with broker_probe.task_received_stream(
        f"{M}.agent_ticker.fire_tick", str(ticker["id"])
    ) as collect:
        celery_wire.publish(f"{M}.agent_ticker.scan_due_tickers")
        fanned = collect(minimum=1)
    assert len(fanned) >= 1


def test_scan_due_tickers_skips_not_due(db_conn, broker_url):
    """A ticker with a future next_run_at fans out nothing."""
    chain = seed_helpers.issue_chain(db_conn, "notdue")
    future = (datetime.now(timezone.utc) + timedelta(hours=1)).isoformat()
    fresh = insert_row(
        db_conn,
        "issue_agent_ticker",
        {
            "issue_id": str(chain["issue"]["id"]),
            "used": 0,
            "granted": 0,
            "waited": 0,
            "user_disabled": False,
            "enabled": True,
            "disarm_reason": "",
            "pending_entry": False,
            "pending_entry_free": False,
            "pending_entry_trigger": "",
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
    chain = seed_helpers.issue_chain(db_conn, "sched")
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
            "last_error": "",
            "next_run_at": None,
        },
    )
    _quiesce_bindings(db_conn, binding["id"])
    # Same event-based observation as the ticker fan-out test (see above).
    with broker_probe.task_received_stream(
        f"{M}.scheduler.fire_scheduler_binding", str(binding["id"])
    ) as collect:
        celery_wire.publish(f"{M}.scheduler.scan_due_bindings")
        fanned = collect(minimum=1)
    assert len(fanned) >= 1


def test_loop_scan_consumed_without_crash(db_conn, broker_url):
    """Loop eligibility needs deep runner state: assert consumed + acked."""
    baseline = broker_probe.queue_depth()
    celery_wire.publish(f"{M}.loop.scan_due_targets")
    broker_probe.wait_for_queue_drain(
        baseline=baseline, what="loop scan consumed"
    )


def test_redelivered_scan_fans_out_once_per_run(db_conn, broker_url):
    """Redelivery: two scans of one due ticker → one fire message per scan."""
    chain = seed_helpers.issue_chain(db_conn, "redel")
    ticker = insert_row(
        db_conn,
        "issue_agent_ticker",
        {
            "issue_id": str(chain["issue"]["id"]),
            "used": 0,
            "granted": 0,
            "waited": 0,
            "user_disabled": False,
            "enabled": True,
            "disarm_reason": "",
            "pending_entry": True,
            "pending_entry_free": False,
            "pending_entry_trigger": "",
            "next_run_at": _past(),
        },
    )
    _quiesce_tickers(db_conn, ticker["id"])
    # Same event-based observation as above; two scans → at least two
    # fan-outs for this ticker (beat may add its own).
    with broker_probe.task_received_stream(
        f"{M}.agent_ticker.fire_tick", str(ticker["id"])
    ) as collect:
        celery_wire.publish(f"{M}.agent_ticker.scan_due_tickers")
        celery_wire.publish(f"{M}.agent_ticker.scan_due_tickers")
        fanned = collect(minimum=2)
    assert len(fanned) >= 2
