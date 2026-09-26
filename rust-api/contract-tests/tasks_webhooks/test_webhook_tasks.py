"""D-08 oracle: webhooks + activity + logging tasks.

Django sources: pi_dash/bgtasks/{webhook_task, issue_activities_task,
issue_automation_task, work_item_link_task, recent_visited_task,
page_transaction_task, logger_task, event_tracking_task}.py

Retry note: ``webhook_send_task`` autoretries with ``retry_backoff=600`` —
a full backoff cycle cannot run inside a contract suite, so multi-attempt
timing is pinned by options parity while the suite behaviorally covers the
first-attempt failure log row, the success path, and redelivery.
"""

import json
import uuid

import pytest

from _harness import broker_probe, celery_wire, config, taskspec
from _harness import seed as seed_helpers
from _harness.db import diff, snapshot, wait_for

M = "pi_dash.bgtasks"

WEBHOOK_TASKS = {
    f"{M}.webhook_task.send_webhook_deactivation_email": (
        "pi_dash/bgtasks/webhook_task.py",
        "send_webhook_deactivation_email",
    ),
    f"{M}.webhook_task.webhook_send_task": (
        "pi_dash/bgtasks/webhook_task.py",
        "webhook_send_task",
    ),
    f"{M}.webhook_task.webhook_activity": (
        "pi_dash/bgtasks/webhook_task.py",
        "webhook_activity",
    ),
    f"{M}.webhook_task.model_activity": (
        "pi_dash/bgtasks/webhook_task.py",
        "model_activity",
    ),
    f"{M}.issue_activities_task.issue_activity": (
        "pi_dash/bgtasks/issue_activities_task.py",
        "issue_activity",
    ),
    f"{M}.issue_automation_task.archive_and_close_old_issues": (
        "pi_dash/bgtasks/issue_automation_task.py",
        "archive_and_close_old_issues",
    ),
    f"{M}.work_item_link_task.crawl_work_item_link_title": (
        "pi_dash/bgtasks/work_item_link_task.py",
        "crawl_work_item_link_title",
    ),
    f"{M}.recent_visited_task.recent_visited_task": (
        "pi_dash/bgtasks/recent_visited_task.py",
        "recent_visited_task",
    ),
    f"{M}.page_transaction_task.page_transaction": (
        "pi_dash/bgtasks/page_transaction_task.py",
        "page_transaction",
    ),
    f"{M}.logger_task.process_logs": (
        "pi_dash/bgtasks/logger_task.py",
        "process_logs",
    ),
    f"{M}.event_tracking_task.track_event": (
        "pi_dash/bgtasks/event_tracking_task.py",
        "track_event",
    ),
}


@pytest.mark.parametrize("task_name", sorted(WEBHOOK_TASKS))
def test_wire_payload_parity(task_name):
    message = celery_wire.capture_wire_message(task_name, args=["probe"], kwargs={})
    celery_wire.assert_wire_message(message, task_name, args=["probe"], kwargs={})


def test_retry_options_parity():
    """Retry parity: only webhook_send_task carries retry config."""
    taskspec.assert_task_options(
        "pi_dash/bgtasks/webhook_task.py",
        "webhook_send_task",
        {
            "bind": "True",
            "autoretry_for": "(requests.RequestException,)",
            "retry_backoff": "600",
            "max_retries": "5",
            "retry_jitter": "True",
        },
    )
    for task_name, (module, func) in WEBHOOK_TASKS.items():
        if func == "webhook_send_task":
            continue
        for keyword in ("autoretry_for", "retry_backoff", "max_retries"):
            taskspec.assert_no_option(module, func, keyword)


def test_no_beat_entries_owned():
    """D-08 owns no beat schedule: cleanup owns the webhook-log delete."""
    from _harness.taskspec import load_beat_schedule

    entries = load_beat_schedule()
    owned = [name for name, e in entries.items() if ".webhook_task." in (e["task"] or "")]
    assert owned == [], f"D-08 unexpectedly owns beat entries: {owned}"


def test_fan_out_call_sites():
    taskspec.assert_calls_delay(
        "pi_dash/bgtasks/webhook_task.py", "webhook_activity", "webhook_send_task"
    )
    taskspec.assert_calls_delay(
        "pi_dash/bgtasks/webhook_task.py", "model_activity", "webhook_activity"
    )


def test_worker_registration(broker_url):
    broker_probe.wait_for_registration(set(WEBHOOK_TASKS))


def _seed_workspace_with_hook(db_conn, webhook_sink, **flags):
    owner = seed_helpers.user(db_conn, "contract-hook-owner")
    workspace = seed_helpers.workspace(db_conn, "contracthook", owner["id"])
    url = f"{config.WEBHOOK_SINK_BASE}/hook/{uuid.uuid4().hex}"
    hook = seed_helpers.webhook(db_conn, workspace["id"], url, issue=True, **flags)
    hook = dict(hook)
    hook["workspace_slug"] = workspace["slug"]
    hook["owner_id"] = owner["id"]
    return hook


def test_webhook_send_success(db_conn, broker_url, webhook_sink):
    """enqueue → worker POSTs → sink receipt + webhook_logs DB row."""
    hook = _seed_workspace_with_hook(db_conn, webhook_sink)
    webhook_sink.clear()
    before = snapshot(db_conn, ["webhook_logs"])

    celery_wire.publish(
        f"{M}.webhook_task.webhook_send_task",
        kwargs={
            "webhook_id": str(hook["id"]),
            "slug": hook["workspace_slug"],
            "event": "issue",
            "event_data": {"id": "contract-probe"},
            "action": "POST",
            "current_site": "example.com",
            "activity": None,
        },
    )
    deliveries = webhook_sink.wait_for_count(1, what="webhook POST")
    assert len(deliveries) == 1
    delivery = deliveries[0]
    # Live Django bug: webhook_send_task names its custom headers with
    # spaces ("X-Pi Dash-Event", ...), which requests/urllib3 silently
    # drops — the POST arrives without them (verified wire-level against
    # requests 2.32/2.33). The Rust port must reproduce the wire bytes,
    # so the oracle pins absence here, not presence.
    assert delivery["headers"].get("Content-Type") == "application/json"
    assert delivery["headers"].get("User-Agent") == "Autopilot"
    assert "X-Pi Dash-Event" not in delivery["headers"]
    assert "X-Pi Dash-Delivery" not in delivery["headers"]
    assert "X-Pi Dash-Signature" not in delivery["headers"]
    payload = json.loads(delivery["body"])
    assert payload["event"] == "issue"
    assert payload["action"] == "create"  # POST maps to create
    assert payload["data"] == {"id": "contract-probe"}

    new_rows = wait_for(
        lambda: _log_diff_rows(db_conn, before) or None,
        what="webhook_logs row",
    )
    assert len(new_rows) == 1
    assert str(new_rows[0]["response_status"]) == "200"
    assert new_rows[0]["retry_count"] == 0
    # ...while the logged request_headers show the task did compute them
    # (dropped at transmission, not at construction).
    assert "X-Pi Dash-Event" in new_rows[0]["request_headers"]


def test_webhook_send_failure_logs_retry_count(db_conn, broker_url, webhook_sink):
    """First-attempt failure: 500 from the sink → log row with retry_count 0."""
    hook = _seed_workspace_with_hook(db_conn, webhook_sink)
    webhook_sink.clear()
    webhook_sink.fail_next(100)
    before = snapshot(db_conn, ["webhook_logs"])

    celery_wire.publish(
        f"{M}.webhook_task.webhook_send_task",
        kwargs={
            "webhook_id": str(hook["id"]),
            "slug": hook["workspace_slug"],
            "event": "issue",
            "event_data": {"id": "contract-fail"},
            "action": "POST",
            "current_site": "example.com",
            "activity": None,
        },
    )
    webhook_sink.wait_for_count(1, what="failed webhook POST")
    after = wait_for(
        lambda: _log_diff_rows(db_conn, before) or None,
        what="failure webhook_logs row",
    )
    assert after[0]["retry_count"] == 0
    assert str(after[0]["response_status"]) == "500"


def test_webhook_redelivery_posts_twice_with_identical_bodies(
    db_conn, broker_url, webhook_sink
):
    """Redelivery: two publishes → two POSTs with identical bodies.

    Django intends distinct per-execution delivery ids, but the
    ``X-Pi Dash-Delivery`` header never reaches the wire (see
    test_webhook_send_success), so redeliveries are header-identical —
    parity is two POSTs, same body.
    """
    hook = _seed_workspace_with_hook(db_conn, webhook_sink)
    webhook_sink.clear()
    kwargs = {
        "webhook_id": str(hook["id"]),
        "slug": hook["workspace_slug"],
        "event": "issue",
        "event_data": {"id": "contract-redelivery"},
        "action": "POST",
        "current_site": "example.com",
        "activity": None,
    }
    celery_wire.publish(f"{M}.webhook_task.webhook_send_task", kwargs=kwargs)
    celery_wire.publish(f"{M}.webhook_task.webhook_send_task", kwargs=kwargs)
    deliveries = webhook_sink.wait_for_count(2, what="redelivered webhook POSTs")
    assert all("X-Pi Dash-Delivery" not in d["headers"] for d in deliveries)
    bodies = [json.loads(d["body"])["data"] for d in deliveries]
    assert bodies == [{"id": "contract-redelivery"}] * 2


def test_activity_chain_fans_out_to_sink(db_conn, broker_url, webhook_sink):
    """model_activity → webhook_activity → webhook_send_task → sink POST."""
    # model_id must be a real issue: webhook_activity serializes it via
    # get_model_data, and a miss raises ObjectDoesNotExist which the
    # chain swallows without fanning out (no POST).
    chain = seed_helpers.issue_chain(db_conn, "chain")
    owner, workspace, issue = chain["owner"], chain["workspace"], chain["issue"]
    url = f"{config.WEBHOOK_SINK_BASE}/hook/{uuid.uuid4().hex}"
    seed_helpers.webhook(db_conn, workspace["id"], url, issue=True)
    webhook_sink.clear()

    celery_wire.publish(
        f"{M}.webhook_task.model_activity",
        args=[
            "issue",
            str(issue["id"]),
            {},
            None,
            str(owner["id"]),
            workspace["slug"],
            "example.com",
        ],
    )
    deliveries = webhook_sink.wait_for_count(1, what="chained webhook POST")
    payload = json.loads(deliveries[0]["body"])
    assert payload["event"] == "issue"


def test_deactivation_email_delivers(db_conn, broker_url, smtp_sink):
    owner = seed_helpers.user(db_conn, "contract-deact-owner")
    receiver = seed_helpers.user(db_conn, "contract-deact-receiver")
    workspace = seed_helpers.workspace(db_conn, "contractdeact", owner["id"])
    hook = seed_helpers.webhook(
        db_conn, workspace["id"], "https://example.com/hook", created_by_id=str(owner["id"])
    )
    smtp_sink.clear()
    celery_wire.publish(
        f"{M}.webhook_task.send_webhook_deactivation_email",
        args=[str(hook["id"]), str(receiver["id"]), "example.com", "contract reason"],
    )
    delivered = smtp_sink.wait_for_count(1, what="deactivation mail")
    assert any(receiver["email"] in m["rcpt_tos"] for m in delivered)


def test_process_logs_writes_postgres_row(db_conn, broker_url):
    """Without Mongo configured, process_logs falls back to Postgres."""
    before = snapshot(db_conn, ["api_activity_logs"])
    celery_wire.publish(
        f"{M}.logger_task.process_logs",
        args=[
            {
                "token_identifier": "contract-probe",
                "path": "/contract/probe",
                "method": "GET",
                "response_code": 200,
            },
            {},
        ],
    )
    after = wait_for(
        lambda: _added_rows(db_conn, "api_activity_logs", before) or None,
        what="api_activity_logs row",
    )
    assert after[0]["path"] == "/contract/probe"


def test_track_event_consumed_without_crash(db_conn, broker_url):
    """No PostHog configured → early return; the job must still be consumed."""
    baseline = broker_probe.queue_depth()
    celery_wire.publish(
        f"{M}.event_tracking_task.track_event",
        args=[str(uuid.uuid4()), "contract.probe", "contract-ws", {}],
    )
    broker_probe.wait_for_queue_drain(
        baseline=baseline, what="track_event consumed"
    )


def test_light_tasks_consumed_without_crash(db_conn, broker_url):
    """Execution parity: every remaining D-08 task is consumed and acked."""
    cases = [
        (
            f"{M}.issue_automation_task.archive_and_close_old_issues",
            [],
            {},
        ),
        (
            f"{M}.page_transaction_task.page_transaction",
            ["<p>new</p>", "<p>old</p>", str(uuid.uuid4())],
            {},
        ),
        (
            f"{M}.recent_visited_task.recent_visited_task",
            ["issue", str(uuid.uuid4()), str(uuid.uuid4()), str(uuid.uuid4()), "ws"],
            {},
        ),
    ]
    baseline = broker_probe.queue_depth()
    for task_name, args, kwargs in cases:
        celery_wire.publish(task_name, args=args, kwargs=kwargs)
    broker_probe.wait_for_queue_drain(
        baseline=baseline, what="light D-08 tasks consumed"
    )


def _log_diff_rows(conn, before):
    after = snapshot(conn, ["webhook_logs"])["webhook_logs"]
    new_keys = set(after) - set(before["webhook_logs"])
    return [after[k] for k in new_keys]


def _added_rows(conn, table, before):
    after = snapshot(conn, [table])[table]
    new_keys = set(after) - set(before[table])
    return [after[k] for k in new_keys]
