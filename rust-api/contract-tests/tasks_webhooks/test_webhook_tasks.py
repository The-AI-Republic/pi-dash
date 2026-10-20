"""D-08 oracle: webhooks + activity + logging tasks.

Django sources: pi_dash/bgtasks/{webhook_task, issue_activities_task,
issue_automation_task, work_item_link_task, recent_visited_task,
page_transaction_task, logger_task, event_tracking_task}.py

Retry note: ``webhook_send_task`` autoretries with ``retry_backoff=600`` —
a full backoff cycle cannot run inside a contract suite, so multi-attempt
timing is pinned by options parity while the suite behaviorally covers the
first-attempt failure log row, the success path, and redelivery.
"""

import ast
import hashlib
import hmac
import json
import os
import socket
import uuid

import pytest

from _harness import broker_probe, celery_wire, config, taskspec
from _harness import seed as seed_helpers
from _harness.db import diff, snapshot, wait_for_condition as wait_for
from _harness.sinks import WildcardHttpProbe

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


#: Tasks the live Django worker registers (Celery broadcast inspect). NOTE
#: (PIDASHCONV-242, FX-LOG-01): ``pi_dash.bgtasks.logger_task.process_logs``
#: IS registered in the canonical worker-plane env
#: (``DJANGO_SETTINGS_MODULE=pi_dash.settings.local``). The module is in
#: neither ``CELERY_IMPORTS`` (settings/common.py:409) nor
#: ``bgtasks/apps.py ready()``; its only static importer is request-time
#: ``middleware/logger.py:19``. Under local settings, though,
#: ``debug_toolbar``'s ``check_middleware`` system check import-strings every
#: ``MIDDLEWARE`` entry during ``celery worker`` boot
#: (``DjangoWorkerFixup.validate_models`` → ``run_checks()``), which imports
#: the middleware module and registers the ``@shared_task`` as a side
#: effect — so the worker answers inspect with ``process_logs`` present and
#: executes published jobs (``api_activity_logs`` row written). Stacks
#: booted without ``debug_toolbar`` (e.g. test settings) see the task
#: absent; that env is non-canonical for this suite. The Rust worker matches
#: execution (parity decision in the PIDASHCONV-242 workpad/PR; gate
#: PIDASHCONV-203 must register ``PROCESS_LOGS_TASK`` when wiring D-08).


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
    # The dropped headers take the body with them (verified wire-level):
    # Django transmits an empty POST. The intended payload survives only
    # in the logged request_body below.
    assert delivery["body"] == ""

    new_rows = wait_for(
        lambda: _log_diff_rows(db_conn, before) or None,
        what="webhook_logs row",
    )
    assert len(new_rows) == 1
    assert str(new_rows[0]["response_status"]) == "200"
    assert new_rows[0]["retry_count"] == 0
    # The logged method is the mapped action (POST -> create), not the wire verb.
    assert new_rows[0]["request_method"] == "create"
    # ...while the logged request_headers show the task did compute them
    # (dropped at transmission, not at construction).
    assert "X-Pi Dash-Event" in new_rows[0]["request_headers"]
    # ...and the logged request_body carries the intended JSON payload
    # (POST maps to create).
    assert "contract-probe" in new_rows[0]["request_body"]
    assert "create" in new_rows[0]["request_body"]


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
    before = snapshot(db_conn, ["webhook_logs"])
    celery_wire.publish(f"{M}.webhook_task.webhook_send_task", kwargs=kwargs)
    celery_wire.publish(f"{M}.webhook_task.webhook_send_task", kwargs=kwargs)
    deliveries = webhook_sink.wait_for_count(2, what="redelivered webhook POSTs")
    assert all("X-Pi Dash-Delivery" not in d["headers"] for d in deliveries)
    assert [d["body"] for d in deliveries] == [""] * 2
    logged = wait_for(
        lambda: _logs_with_count(db_conn, before, 2),
        what="two redelivered webhook_logs rows",
    )
    assert all(str(r["response_status"]) == "200" for r in logged)


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
    before = snapshot(db_conn, ["webhook_logs"])

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
    assert deliveries[0]["body"] == ""
    chained = wait_for(
        lambda: _log_diff_rows(db_conn, before) or None,
        what="chained webhook_logs row",
    )
    assert chained[0]["event_type"] == "issue"


def _soft_delete_row(conn, table, row_id):
    """Stamp ``deleted_at`` on one seeded row.

    Every read below goes through ``Model.objects`` (the inherited
    ``SoftDeletionManager``, ``pi_dash/db/mixins.py:65``), so a stamped
    row is invisible to the Django tasks. These tests are the oracle-side
    half of the soft-delete regression pins; the replay suite carries the
    matching Rust-side cases (``*_soft_deleted_*`` in
    ``test_rust_replay.py``).
    """
    with conn.cursor() as cur:
        cur.execute(
            f"UPDATE {table} SET deleted_at = now() WHERE id = %s",
            (str(row_id),),
        )


def test_webhook_send_skips_soft_deleted_webhook(db_conn, broker_url, webhook_sink):
    """Send task, Django side: a soft-deleted webhook is swallowed.

    ``Webhook.objects.get(id=..., workspace__slug=...)`` hides the row, so
    the task returns before POSTing or logging — no sink receipt, no
    ``webhook_logs`` row.
    """
    hook = _seed_workspace_with_hook(db_conn, webhook_sink)
    _soft_delete_row(db_conn, "webhooks", hook["id"])
    webhook_sink.clear()
    before = snapshot(db_conn, ["webhook_logs"])
    baseline = broker_probe.queue_depth()
    celery_wire.publish(
        f"{M}.webhook_task.webhook_send_task",
        kwargs={
            "webhook_id": str(hook["id"]),
            "slug": hook["workspace_slug"],
            "event": "issue",
            "event_data": {"id": "contract-dead-hook"},
            "action": "POST",
            "current_site": "example.com",
            "activity": None,
        },
    )
    broker_probe.wait_for_queue_drain(
        baseline=baseline, what="send task consumed for a soft-deleted webhook"
    )
    assert webhook_sink.snapshot() == [], "send POSTed to a soft-deleted webhook"
    assert _log_diff_rows(db_conn, before) == [], "send logged a soft-deleted webhook"


def test_activity_skips_soft_deleted_webhook(db_conn, broker_url, webhook_sink):
    """Fan-out, Django side: soft-deleted webhooks get no send job."""
    chain = seed_helpers.issue_chain(db_conn, "chaindeadhook")
    owner, workspace, issue = chain["owner"], chain["workspace"], chain["issue"]
    url = f"{config.WEBHOOK_SINK_BASE}/hook/{uuid.uuid4().hex}"
    hook = seed_helpers.webhook(db_conn, workspace["id"], url, issue=True)
    _soft_delete_row(db_conn, "webhooks", hook["id"])
    webhook_sink.clear()
    before = snapshot(db_conn, ["webhook_logs"])
    baseline = broker_probe.queue_depth()
    celery_wire.publish(
        f"{M}.webhook_task.webhook_activity",
        kwargs={
            "event": "issue",
            "verb": "created",
            "field": None,
            "old_value": None,
            "new_value": None,
            "actor_id": str(owner["id"]),
            "slug": workspace["slug"],
            "current_site": "example.com",
            "event_id": str(issue["id"]),
            "old_identifier": None,
            "new_identifier": None,
        },
    )
    broker_probe.wait_for_queue_drain(
        baseline=baseline, what="fan-out consumed with only a soft-deleted hook"
    )
    assert webhook_sink.snapshot() == [], "fan-out notified a soft-deleted webhook"
    assert _log_diff_rows(db_conn, before) == [], "fan-out logged a deleted hook"


def test_activity_swallows_soft_deleted_issue(db_conn, broker_url, webhook_sink):
    """Fan-out, Django side: a soft-deleted event model is swallowed."""
    chain = seed_helpers.issue_chain(db_conn, "chaindeadissue")
    owner, workspace, issue = chain["owner"], chain["workspace"], chain["issue"]
    url = f"{config.WEBHOOK_SINK_BASE}/hook/{uuid.uuid4().hex}"
    seed_helpers.webhook(db_conn, workspace["id"], url, issue=True)
    _soft_delete_row(db_conn, "issues", issue["id"])
    webhook_sink.clear()
    before = snapshot(db_conn, ["webhook_logs"])
    baseline = broker_probe.queue_depth()
    celery_wire.publish(
        f"{M}.webhook_task.webhook_activity",
        kwargs={
            "event": "issue",
            "verb": "created",
            "field": None,
            "old_value": None,
            "new_value": None,
            "actor_id": str(owner["id"]),
            "slug": workspace["slug"],
            "current_site": "example.com",
            "event_id": str(issue["id"]),
            "old_identifier": None,
            "new_identifier": None,
        },
    )
    broker_probe.wait_for_queue_drain(
        baseline=baseline, what="fan-out consumed for a soft-deleted issue"
    )
    assert webhook_sink.snapshot() == [], "fan-out notified a soft-deleted issue"
    assert _log_diff_rows(db_conn, before) == [], "fan-out logged a deleted issue"


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
    assert any(m["subject"] == "Webhook Deactivated" for m in delivered)


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


def test_track_event_posts_capture_to_posthog(db_conn, broker_url, posthog_sink):
    """Configured PostHog → the worker POSTs the /batch/ capture (PIDASHCONV-1107)."""
    owner = seed_helpers.user(db_conn, "contract-evt-owner")
    workspace = seed_helpers.workspace(db_conn, "contractevt", owner["id"])
    posthog_sink.clear()
    celery_wire.publish(
        f"{M}.event_tracking_task.track_event",
        args=[str(owner["id"]), "contract.probe", workspace["slug"], {"source": "contract"}],
    )
    (capture,) = posthog_sink.wait_for_count(1, what="posthog /batch/ capture")
    assert capture["path"] == "/batch/"
    # Header names are case-insensitive on the wire (RFC 9110 §5.1):
    # compare lowercased so the SDK (requests) and the port (reqwest)
    # pin the same header set despite different capitalisation.
    headers = {key.lower(): value for key, value in capture["headers"].items()}
    assert headers.get("content-type") == "application/json"
    assert headers.get("user-agent") == "posthog-python/3.5.0"
    body = capture["json"]
    assert body["api_key"] == os.environ["POSTHOG_API_KEY"]
    assert body["sentAt"]
    assert body["historical_migration"] is False
    (msg,) = body["batch"]
    assert msg["event"] == "contract.probe"
    assert msg["distinct_id"] == str(owner["id"])
    assert msg["context"] == {}
    assert msg["timestamp"]
    assert msg["properties"]["source"] == "contract"
    assert msg["properties"]["$groups"] == {"workspace": workspace["slug"]}
    assert msg["properties"]["$lib"] == "posthog-python"
    assert msg["properties"]["$lib_version"] == "3.5.0"
    assert msg["properties"]["$geoip_disable"] is True


def test_track_event_assigns_workspace_role(db_conn, broker_url, posthog_sink):
    """Role events stamp owner/admin/unknown from the workspace lookup."""
    owner = seed_helpers.user(db_conn, "contract-role-owner")
    member = seed_helpers.user(db_conn, "contract-role-member")
    workspace = seed_helpers.workspace(db_conn, "contractrole", owner["id"])
    missing_slug = f"missing-ws-{uuid.uuid4().hex[:8]}"
    posthog_sink.clear()
    cases = [
        (str(owner["id"]), workspace["slug"], "owner"),
        (str(member["id"]), workspace["slug"], "admin"),
        (str(owner["id"]), missing_slug, "unknown"),
    ]
    for user_id, slug, role in cases:
        celery_wire.publish(
            f"{M}.event_tracking_task.track_event",
            args=[user_id, "user_invited_to_workspace", slug, {"case": role}],
        )
    captures = posthog_sink.wait_for_count(3, what="role capture batch")
    roles = {
        capture["json"]["batch"][0]["properties"]["case"]: capture["json"]["batch"][0][
            "properties"
        ]["role"]
        for capture in captures
    }
    assert roles == {"owner": "owner", "admin": "admin", "unknown": "unknown"}


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


def test_recent_visited_writes_row(db_conn, broker_url):
    """Create path inserts the visit row; repeat visit touches visited_at (PIDASHCONV-1107)."""
    from _harness.db import insert_row

    owner = seed_helpers.user(db_conn, "contract-visit-owner")
    workspace = seed_helpers.workspace(db_conn, "contractvisit", owner["id"])
    project = seed_helpers.project(db_conn, workspace["id"])
    entity_id = str(uuid.uuid4())
    before = snapshot(db_conn, ["user_recent_visits"])
    celery_wire.publish(
        f"{M}.recent_visited_task.recent_visited_task",
        args=["issue", entity_id, str(owner["id"]), str(project["id"]), workspace["slug"]],
    )
    (created,) = wait_for(
        lambda: _added_rows(db_conn, "user_recent_visits", before) or None,
        what="user_recent_visits row",
    )
    assert created["entity_name"] == "issue"
    assert created["entity_identifier"] == entity_id
    assert created["user_id"] == str(owner["id"])
    assert created["project_id"] == str(project["id"])
    assert created["workspace_id"] == str(workspace["id"])
    assert created["visited_at"] is not None
    # QUIRK-5: the create-path backfill save() re-stamps audit from the
    # worker's (absent) crum user, so both audit columns stay NULL.
    assert created["created_by_id"] is None
    assert created["updated_by_id"] is None

    celery_wire.publish(
        f"{M}.recent_visited_task.recent_visited_task",
        args=["issue", entity_id, str(owner["id"]), str(project["id"]), workspace["slug"]],
    )

    def _touched():
        row = snapshot(db_conn, ["user_recent_visits"])["user_recent_visits"].get(
            str(created["id"])
        )
        return row if row and row["visited_at"] != created["visited_at"] else None

    touched = wait_for(_touched, what="visited_at touch")
    assert touched["visited_at"] > created["visited_at"]
    live = [
        row
        for row in snapshot(db_conn, ["user_recent_visits"])["user_recent_visits"].values()
        if row["user_id"] == str(owner["id"]) and row["deleted_at"] is None
    ]
    assert len(live) == 1
    # update_fields=["visited_at"] writes only that column: updated_at stays.
    assert touched["updated_at"] == created["updated_at"]


def _seed_page(db_conn, workspace_id, owner_id):
    from _harness.db import insert_row

    return insert_row(
        db_conn,
        "pages",
        {
            "name": "Contract tx page",
            "description_json": {},
            "description_html": "<p></p>",
            "owned_by_id": str(owner_id),
            "access": 0,
            "workspace_id": str(workspace_id),
            "color": "",
            "is_locked": False,
            "view_props": {"full_width": False},
            "logo_props": {},
            "is_global": False,
            "sort_order": 65535,
            "created_by_id": str(owner_id),
        },
    )


def _live_page_logs(db_conn, page_id):
    return [
        row
        for row in snapshot(db_conn, ["page_logs"])["page_logs"].values()
        if row["page_id"] == str(page_id) and row["deleted_at"] is None
    ]


def test_page_transaction_writes_page_logs(db_conn, broker_url):
    """New components insert PageLog rows; removed ones soft-delete (PIDASHCONV-1107)."""
    owner = seed_helpers.user(db_conn, "contract-tx-owner")
    workspace = seed_helpers.workspace(db_conn, "contracttx", owner["id"])
    page = _seed_page(db_conn, workspace["id"], owner["id"])
    mention_id, image_id, entity_id = (str(uuid.uuid4()) for _ in range(3))
    # The image src is a UUID string: a URL src poisons the whole
    # bulk_create (entity_identifier is a UUID column), pinned below in
    # test_page_transaction_url_src_inserts_nothing.
    html = (
        f'<mention-component id="{mention_id}" entity_identifier="{entity_id}" '
        f'entity_name="ISS-1" entity_type="issue"></mention-component>'
        f'<image-component id="{image_id}" src="{entity_id}"></image-component>'
    )
    celery_wire.publish(
        f"{M}.page_transaction_task.page_transaction",
        args=[html, "", str(page["id"])],
    )
    rows = wait_for(
        lambda: _live_page_logs(db_conn, page["id"]) or None,
        what="page_logs rows",
    )
    assert len(rows) == 2
    by_tx = {row["transaction"]: row for row in rows}
    mention = by_tx[mention_id]
    assert mention["entity_name"] == "ISS-1"
    # The mention extract reads the entity_type attribute but drops it.
    assert mention["entity_type"] is None
    assert mention["entity_identifier"] == entity_id
    assert mention["workspace_id"] == str(workspace["id"])
    image = by_tx[image_id]
    assert image["entity_name"] == "image"
    assert image["entity_type"] is None
    assert image["entity_identifier"] == entity_id

    celery_wire.publish(
        f"{M}.page_transaction_task.page_transaction",
        args=["", html, str(page["id"])],
    )
    wait_for(
        lambda: True if _live_page_logs(db_conn, page["id"]) == [] else None,
        what="page_logs soft-delete",
    )
    deleted = [
        row
        for row in snapshot(db_conn, ["page_logs"])["page_logs"].values()
        if row["page_id"] == str(page["id"])
    ]
    assert len(deleted) == 2
    assert all(row["deleted_at"] is not None for row in deleted)


def test_page_transaction_url_src_inserts_nothing(db_conn, broker_url):
    """A URL image src poisons the batch: the UUID column rejects it (PIDASHCONV-1107).

    Live Django bug (ported): ``entity_identifier`` is a UUID column but
    the image extract passes the raw ``src`` URL, so ``bulk_create``
    raises and the outer handler swallows it — the mention row in the
    same batch is lost too, and the job still acks.

    Completion signal: the suite's worker runs ``--pool=solo`` (strictly
    serial, FIFO), so a valid marker job published after the poisoned one
    proves the poisoned run finished once the marker's row lands.
    """
    owner = seed_helpers.user(db_conn, "contract-txbug-owner")
    workspace = seed_helpers.workspace(db_conn, "contracttxbug", owner["id"])
    page = _seed_page(db_conn, workspace["id"], owner["id"])
    marker_page = _seed_page(db_conn, workspace["id"], owner["id"])
    mention_id = str(uuid.uuid4())
    html = (
        f'<mention-component id="{mention_id}" entity_identifier="{uuid.uuid4()}" '
        f'entity_name="ISS-1" entity_type="issue"></mention-component>'
        f'<image-component id="{uuid.uuid4()}" src="https://cdn.example/logo.png">'
        f"</image-component>"
    )
    before = snapshot(db_conn, ["page_logs"])
    celery_wire.publish(
        f"{M}.page_transaction_task.page_transaction",
        args=[html, "", str(page["id"])],
    )
    marker_id = str(uuid.uuid4())
    marker_html = (
        f'<mention-component id="{marker_id}" entity_identifier="{uuid.uuid4()}" '
        f'entity_name="M-1" entity_type="issue"></mention-component>'
    )
    celery_wire.publish(
        f"{M}.page_transaction_task.page_transaction",
        args=[marker_html, "", str(marker_page["id"])],
    )
    wait_for(
        lambda: _live_page_logs(db_conn, marker_page["id"]) or None,
        what="marker page_logs row",
    )
    assert _added_rows(db_conn, "page_logs", before) != []
    poisoned = [
        row
        for row in snapshot(db_conn, ["page_logs"])["page_logs"].values()
        if row["page_id"] == str(page["id"])
    ]
    assert poisoned == []


#: ``data:image/svg+xml;base64,`` + DEFAULT_FAVICON
#: (``work_item_link_task.py:24``), the favicon fallback.
DEFAULT_FAVICON_DATA_URI = (
    "data:image/svg+xml;base64,"
    "PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIyNCIgaGVpZ2h0PSIyNCIg"
    "dmlld0JveD0iMCAwIDI0IDI0IiBmaWxsPSJub25lIiBzdHJva2U9ImN1cnJlbnRDb2xvciIgc3Ryb2tlLXdp"
    "ZHRoPSIyIiBzdHJva2UtbGluZWNhcD0icm91bmQiIHN0cm9rZS1saW5lam9pbj0icm91bmQiIGNsYXNzPSJs"
    "dWNpZGUgbHVjaWRlLWxpbmstaWNvbiBsdWNpZGUtbGluayI+PHBhdGggZD0iTTEwIDEzYTUgNSAwIDAgMCA3"
    "LjU0LjU0bDMtM2E1IDUgMCAwIDAtNy4wNy03LjA3bC0xLjcyIDEuNzEiLz48cGF0aCBkPSJNMTQgMTFhNSA1"
    "IDAgMCAwLTcuNTQtLjU0bC0zIDNhNSA1IDAgMCAwIDcuMDcgNy4wN2wxLjcxLTEuNzEiLz48L3N2Zz4="
)


def test_crawl_link_updates_metadata(db_conn, broker_url):
    """Loopback URL: SSRF guard rejects it; metadata keeps the fallback shape (PIDASHCONV-1107)."""
    from _harness.db import insert_row

    chain = seed_helpers.issue_chain(db_conn, "contractcrawl")
    issue, project, workspace = chain["issue"], chain["project"], chain["workspace"]
    url = "http://127.0.0.1:9/contract-probe"
    link = insert_row(
        db_conn,
        "issue_links",
        {
            "title": "Seed probe",
            "url": url,
            "issue_id": str(issue["id"]),
            "project_id": str(project["id"]),
            "workspace_id": str(workspace["id"]),
            "created_by_id": str(chain["owner"]["id"]),
            "updated_by_id": str(chain["owner"]["id"]),
            "metadata": {},
        },
    )
    celery_wire.publish(
        f"{M}.work_item_link_task.crawl_work_item_link_title",
        args=[str(link["id"]), url],
    )

    def _crawled():
        row = snapshot(db_conn, ["issue_links"])["issue_links"].get(str(link["id"]))
        return row if row and row["metadata"] != {} else None

    crawled = wait_for(_crawled, what="issue_links metadata")
    assert crawled["metadata"] == {
        "title": None,
        "favicon": DEFAULT_FAVICON_DATA_URI,
        "url": url,
        "favicon_url": None,
    }


@pytest.mark.parametrize(
    "family",
    [socket.AF_INET, socket.AF_INET6],
    ids=["wildcard-ipv4", "wildcard-ipv6"],
)
def test_crawl_link_denies_wildcard_hosts(db_conn, broker_url, family):
    """Django denies 0.0.0.0 and :: even though they are not literal loopback.

    CPython's ``is_private`` covers both addresses (and the surrounding
    ranges), so the crawler must keep the fallback metadata even when a
    listener is reachable through the wildcard address.
    """
    from _harness.db import insert_row

    with WildcardHttpProbe(family) as probe:
        chain = seed_helpers.issue_chain(
            db_conn,
            "contractwildcard" + ("v6" if family == socket.AF_INET6 else "v4"),
        )
        issue, project, workspace = chain["issue"], chain["project"], chain["workspace"]
        url = probe.url
        link = insert_row(
            db_conn,
            "issue_links",
            {
                "title": "Wildcard probe",
                "url": url,
                "issue_id": str(issue["id"]),
                "project_id": str(project["id"]),
                "workspace_id": str(workspace["id"]),
                "created_by_id": str(chain["owner"]["id"]),
                "updated_by_id": str(chain["owner"]["id"]),
                "metadata": {},
            },
        )
        celery_wire.publish(
            f"{M}.work_item_link_task.crawl_work_item_link_title",
            args=[str(link["id"]), url],
        )

        def _crawled():
            row = snapshot(db_conn, ["issue_links"])["issue_links"].get(str(link["id"]))
            return row if row and row["metadata"] != {} else None

        crawled = wait_for(_crawled, what="issue_links metadata")
        assert crawled["metadata"] == {
            "title": None,
            "favicon": DEFAULT_FAVICON_DATA_URI,
            "url": url,
            "favicon_url": None,
        }


def _expected_payload(hook, probe_id):
    """The delivery envelope the send task signs and POSTs (PIDASHCONV-1105).

    Key order is the source dict order (event, action, webhook_id,
    workspace_id, data, activity); the HMAC input is ``json.dumps`` with
    stdlib defaults over exactly this dict.
    """
    return {
        "event": "issue",
        "action": "create",
        "webhook_id": str(hook["id"]),
        "workspace_id": str(hook["workspace_id"]),
        "data": {"id": probe_id},
        "activity": None,
    }


def _expected_signature(secret, payload):
    return hmac.new(
        secret.encode("utf-8"),
        json.dumps(payload).encode("utf-8"),
        hashlib.sha256,
    ).hexdigest()


def test_webhook_send_signature_logged(db_conn, broker_url, webhook_sink):
    """Fixed secret: the logged signature is HMAC over the exact envelope (PIDASHCONV-1105)."""
    owner = seed_helpers.user(db_conn, "contract-sig-owner")
    workspace = seed_helpers.workspace(db_conn, "contractsig", owner["id"])
    url = f"{config.WEBHOOK_SINK_BASE}/hook/{uuid.uuid4().hex}"
    secret = "contract-secret-1105"
    hook = seed_helpers.webhook(
        db_conn, workspace["id"], url, issue=True, secret_key=secret
    )
    probe_id = "contract-signature"
    payload = _expected_payload(hook, probe_id)
    webhook_sink.clear()
    before = snapshot(db_conn, ["webhook_logs"])

    celery_wire.publish(
        f"{M}.webhook_task.webhook_send_task",
        kwargs={
            "webhook_id": str(hook["id"]),
            "slug": workspace["slug"],
            "event": "issue",
            "event_data": {"id": probe_id},
            "action": "POST",
            "current_site": "example.com",
            "activity": None,
        },
    )
    webhook_sink.wait_for_count(1, what="signed webhook POST")
    (row,) = wait_for(
        lambda: _log_diff_rows(db_conn, before) or None,
        what="signed webhook_logs row",
    )
    logged_headers = ast.literal_eval(row["request_headers"])
    assert list(logged_headers) == [
        "Content-Type",
        "User-Agent",
        "X-Pi Dash-Delivery",
        "X-Pi Dash-Event",
        "X-Pi Dash-Signature",
    ]
    assert logged_headers["X-Pi Dash-Signature"] == _expected_signature(secret, payload)
    logged_body = ast.literal_eval(row["request_body"])
    assert list(logged_body) == [
        "event",
        "action",
        "webhook_id",
        "workspace_id",
        "data",
        "activity",
    ]
    assert logged_body == payload


def test_webhook_send_exhaustion_deactivates(db_conn, broker_url, smtp_sink):
    """Retries exhausted: failure row, webhook disabled, deactivation mail (PIDASHCONV-1105).

    The job lands with ``retries=5`` preset (a redelivered sixth attempt)
    against a refused URL, so the transport error takes the
    ``retries >= max_retries`` arm directly: no retry is scheduled, the
    webhook flips ``is_active`` (``updated_at`` untouched — a filtered
    ``update()``, not a ``save()``), and the deactivation mail goes to
    the webhook creator.
    """
    owner = seed_helpers.user(db_conn, "contract-exhaust-owner")
    workspace = seed_helpers.workspace(db_conn, "contractexhaust", owner["id"])
    hook = seed_helpers.webhook(
        db_conn,
        workspace["id"],
        f"http://127.0.0.1:9/{uuid.uuid4().hex}",
        issue=True,
        created_by_id=str(owner["id"]),
    )
    before_hooks = snapshot(db_conn, ["webhooks"])["webhooks"][str(hook["id"])]
    before = snapshot(db_conn, ["webhook_logs"])
    smtp_sink.clear()

    celery_wire.publish_with_retries(
        f"{M}.webhook_task.webhook_send_task",
        kwargs={
            "webhook_id": str(hook["id"]),
            "slug": workspace["slug"],
            "event": "issue",
            "event_data": {"id": "contract-exhaustion"},
            "action": "POST",
            "current_site": "example.com",
            "activity": None,
        },
        retries=5,
    )
    (row,) = wait_for(
        lambda: _log_diff_rows(db_conn, before) or None,
        what="exhaustion webhook_logs row",
    )
    assert row["retry_count"] == 5
    assert str(row["response_status"]) == "500"
    assert row["response_headers"] == ""
    assert "refused" in row["response_body"].lower()

    def _deactivated():
        current = snapshot(db_conn, ["webhooks"])["webhooks"][str(hook["id"])]
        return current if current["is_active"] is False else None

    deactivated = wait_for(_deactivated, what="webhook deactivation")
    assert deactivated["updated_at"] == before_hooks["updated_at"]

    delivered = smtp_sink.wait_for_count(1, what="exhaustion deactivation mail")
    assert any(owner["email"] in m["rcpt_tos"] for m in delivered)
    assert any(m["subject"] == "Webhook Deactivated" for m in delivered)


def _logs_with_count(conn, before, n):
    rows = _log_diff_rows(conn, before)
    return rows if len(rows) == n else None


def _log_diff_rows(conn, before):
    after = snapshot(conn, ["webhook_logs"])["webhook_logs"]
    new_keys = set(after) - set(before["webhook_logs"])
    return [after[k] for k in new_keys]


def _added_rows(conn, table, before):
    after = snapshot(conn, [table])[table]
    new_keys = set(after) - set(before[table])
    return [after[k] for k in new_keys]
