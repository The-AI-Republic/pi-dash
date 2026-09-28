"""D-09 oracle: cleanup, versions, exports, deletion tasks.

Django sources: pi_dash/bgtasks/{cleanup_task, deletion_task, export_task,
exporter_expired_task, analytic_plot_export, issue_version_sync,
issue_description_version_sync, issue_description_version_task,
page_version_task, file_asset_task, storage_metadata_task, copy_s3_object,
workspace_seed_task, dummy_data_task}.py

Cleanup eligibility is env-driven (``HARD_DELETE_AFTER_DAYS=0``,
``UNUPLOADED_ASSET_DELETE_DAYS=0``); the contract environment sets both.
``issue_export_task`` needs object storage: the contract environment provides
minio behind the Django S3 settings (``USE_MINIO=1``).
"""

import uuid
from datetime import datetime, timedelta, timezone

import pytest
from psycopg.rows import dict_row

from _harness import broker_probe, celery_wire, taskspec
from _harness import seed as seed_helpers
from _harness.db import insert_row, snapshot, wait_for_condition as wait_for

M = "pi_dash.bgtasks"

CLEANUP_TASKS = {
    f"{M}.cleanup_task.delete_api_logs": ("pi_dash/bgtasks/cleanup_task.py", "delete_api_logs"),
    f"{M}.cleanup_task.delete_email_notification_logs": (
        "pi_dash/bgtasks/cleanup_task.py",
        "delete_email_notification_logs",
    ),
    f"{M}.cleanup_task.delete_page_versions": (
        "pi_dash/bgtasks/cleanup_task.py",
        "delete_page_versions",
    ),
    f"{M}.cleanup_task.delete_issue_description_versions": (
        "pi_dash/bgtasks/cleanup_task.py",
        "delete_issue_description_versions",
    ),
    f"{M}.cleanup_task.delete_webhook_logs": (
        "pi_dash/bgtasks/cleanup_task.py",
        "delete_webhook_logs",
    ),
    f"{M}.deletion_task.soft_delete_related_objects": (
        "pi_dash/bgtasks/deletion_task.py",
        "soft_delete_related_objects",
    ),
    f"{M}.deletion_task.hard_delete": ("pi_dash/bgtasks/deletion_task.py", "hard_delete"),
    f"{M}.export_task.issue_export_task": ("pi_dash/bgtasks/export_task.py", "issue_export_task"),
    f"{M}.exporter_expired_task.delete_old_s3_link": (
        "pi_dash/bgtasks/exporter_expired_task.py",
        "delete_old_s3_link",
    ),
    f"{M}.analytic_plot_export.analytic_export_task": (
        "pi_dash/bgtasks/analytic_plot_export.py",
        "analytic_export_task",
    ),
    f"{M}.analytic_plot_export.export_analytics_to_csv_email": (
        "pi_dash/bgtasks/analytic_plot_export.py",
        "export_analytics_to_csv_email",
    ),
    f"{M}.issue_version_sync.issue_task": (
        "pi_dash/bgtasks/issue_version_sync.py",
        "issue_task",
    ),
    f"{M}.issue_version_sync.sync_issue_version": (
        "pi_dash/bgtasks/issue_version_sync.py",
        "sync_issue_version",
    ),
    f"{M}.issue_version_sync.schedule_issue_version": (
        "pi_dash/bgtasks/issue_version_sync.py",
        "schedule_issue_version",
    ),
    f"{M}.issue_description_version_sync.sync_issue_description_version": (
        "pi_dash/bgtasks/issue_description_version_sync.py",
        "sync_issue_description_version",
    ),
    f"{M}.issue_description_version_sync.schedule_issue_description_version": (
        "pi_dash/bgtasks/issue_description_version_sync.py",
        "schedule_issue_description_version",
    ),
    f"{M}.issue_description_version_task.issue_description_version_task": (
        "pi_dash/bgtasks/issue_description_version_task.py",
        "issue_description_version_task",
    ),
    f"{M}.page_version_task.track_page_version": (
        "pi_dash/bgtasks/page_version_task.py",
        "track_page_version",
    ),
    f"{M}.file_asset_task.delete_unuploaded_file_asset": (
        "pi_dash/bgtasks/file_asset_task.py",
        "delete_unuploaded_file_asset",
    ),
    f"{M}.storage_metadata_task.get_asset_object_metadata": (
        "pi_dash/bgtasks/storage_metadata_task.py",
        "get_asset_object_metadata",
    ),
    f"{M}.copy_s3_object.copy_s3_objects_of_description_and_assets": (
        "pi_dash/bgtasks/copy_s3_object.py",
        "copy_s3_objects_of_description_and_assets",
    ),
    f"{M}.workspace_seed_task.workspace_seed": (
        "pi_dash/bgtasks/workspace_seed_task.py",
        "workspace_seed",
    ),
    f"{M}.dummy_data_task.create_dummy_data": (
        "pi_dash/bgtasks/dummy_data_task.py",
        "create_dummy_data",
    ),
}


@pytest.mark.parametrize("task_name", sorted(CLEANUP_TASKS))
def test_wire_payload_parity(task_name):
    message = celery_wire.capture_wire_message(task_name, args=["probe"], kwargs={})
    celery_wire.assert_wire_message(message, task_name, args=["probe"], kwargs={})


@pytest.mark.parametrize("task_name", sorted(CLEANUP_TASKS))
def test_task_options_parity(task_name):
    """Ack parity: plain @shared_task — default ack-on-success, no overrides."""
    module, func = CLEANUP_TASKS[task_name]
    taskspec.assert_task_options(module, func, {})
    for keyword in ("acks_late", "acks_on_failure_or_timeout", "autoretry_for"):
        taskspec.assert_no_option(module, func, keyword)


def test_version_sync_eta_call_sites():
    """ETA parity: the version backfills chain with countdown=300."""
    taskspec.assert_delay_kwarg(
        "pi_dash/bgtasks/issue_version_sync.py",
        "schedule_issue_version",
        "sync_issue_version",
        "countdown",
    )
    taskspec.assert_delay_kwarg(
        "pi_dash/bgtasks/issue_description_version_sync.py",
        "schedule_issue_description_version",
        "sync_issue_description_version",
        "countdown",
    )


def test_beat_entry_parity():
    owned = {
        "check-every-day-to-delete-hard-delete": (
            f"{M}.deletion_task.hard_delete",
            "crontab(hour=0, minute=0)",
        ),
        "check-every-day-to-archive-and-close": (
            f"{M}.issue_automation_task.archive_and_close_old_issues",
            "crontab(hour=1, minute=0)",
        ),
        "check-every-day-to-delete_exporter_history": (
            f"{M}.exporter_expired_task.delete_old_s3_link",
            "crontab(hour=1, minute=30)",
        ),
        "check-every-day-to-delete-file-asset": (
            f"{M}.file_asset_task.delete_unuploaded_file_asset",
            "crontab(hour=2, minute=0)",
        ),
        "check-every-day-to-delete-api-logs": (
            f"{M}.cleanup_task.delete_api_logs",
            "crontab(hour=2, minute=30)",
        ),
        "check-every-day-to-delete-email-notification-logs": (
            f"{M}.cleanup_task.delete_email_notification_logs",
            "crontab(hour=2, minute=45)",
        ),
        "check-every-day-to-delete-page-versions": (
            f"{M}.cleanup_task.delete_page_versions",
            "crontab(hour=3, minute=0)",
        ),
        "check-every-day-to-delete-issue-description-versions": (
            f"{M}.cleanup_task.delete_issue_description_versions",
            "crontab(hour=3, minute=15)",
        ),
        "check-every-day-to-delete-webhook-logs": (
            f"{M}.cleanup_task.delete_webhook_logs",
            "crontab(hour=3, minute=30)",
        ),
        "check-every-day-to-delete-exporter-history": (
            f"{M}.exporter_expired_task.delete_old_s3_link",
            "crontab(hour=3, minute=45)",
        ),
    }
    for entry, (task, schedule) in owned.items():
        taskspec.assert_beat_entry(entry, task, schedule)


def test_worker_registration(broker_url):
    broker_probe.wait_for_registration(set(CLEANUP_TASKS))


def _old(days: int = 60) -> str:
    return (datetime.now(timezone.utc) - timedelta(days=days)).isoformat()


def test_delete_api_logs(db_conn, broker_url):
    row = insert_row(
        db_conn,
        "api_activity_logs",
        {
            "token_identifier": "contract-probe",
            "path": "/contract/old",
            "method": "GET",
            "response_code": 200,
            "created_at": _old(),
        },
    )
    celery_wire.publish(f"{M}.cleanup_task.delete_api_logs")
    wait_for(
        lambda: _missing(db_conn, "api_activity_logs", row["id"]),
        what="aged api_activity_logs row deleted",
    )


def test_delete_email_notification_logs(db_conn, broker_url):
    receiver = seed_helpers.user(db_conn, "contract-expire")
    actor = seed_helpers.user(db_conn, "contract-expire-actor")
    row = seed_helpers.email_log(
        db_conn,
        receiver["id"],
        actor["id"],
        str(uuid.uuid4()),
        sent_at=_old(),
        created_at=_old(),
    )
    celery_wire.publish(f"{M}.cleanup_task.delete_email_notification_logs")
    wait_for(
        lambda: _missing(db_conn, "email_notification_logs", row["id"]),
        what="aged email_notification_logs row deleted",
    )


def test_delete_webhook_logs(db_conn, broker_url):
    owner = seed_helpers.user(db_conn, "contract-weblog-owner")
    workspace = seed_helpers.workspace(db_conn, "contractweblog", owner["id"])
    row = insert_row(
        db_conn,
        "webhook_logs",
        {
            "workspace_id": str(workspace["id"]),
            "webhook": str(uuid.uuid4()),
            "event_type": "issue",
            "retry_count": 0,
            "created_at": _old(),
        },
    )
    celery_wire.publish(f"{M}.cleanup_task.delete_webhook_logs")
    wait_for(
        lambda: _missing(db_conn, "webhook_logs", row["id"]),
        what="aged webhook_logs row deleted",
    )


def test_delete_page_versions_keeps_newest_20(db_conn, broker_url):
    """Windowing parity: only versions beyond the newest 20 per page go."""
    owner = seed_helpers.user(db_conn, "contract-pagever-owner")
    workspace = seed_helpers.workspace(db_conn, "contractpagever", owner["id"])
    page = insert_row(
        db_conn,
        "pages",
        {
            "workspace_id": str(workspace["id"]),
            "owned_by_id": str(owner["id"]),
            "name": "contract page",
            "description_json": {},
            "description_html": "<p></p>",
            "access": 0,
            "color": "",
            "is_locked": False,
            "view_props": {},
            "logo_props": {},
            "is_global": False,
            "sort_order": 65535,
        },
    )
    base = datetime.now(timezone.utc) - timedelta(days=90)
    for i in range(22):
        insert_row(
            db_conn,
            "page_versions",
            {
                "workspace_id": str(workspace["id"]),
                "page_id": str(page["id"]),
                "owned_by_id": str(owner["id"]),
                "description_json": {},
                "description_html": "<p></p>",
                "sub_pages_data": {},
                "last_saved_at": (base + timedelta(minutes=i)).isoformat(),
                "created_at": (base + timedelta(minutes=i)).isoformat(),
            },
        )
    celery_wire.publish(f"{M}.cleanup_task.delete_page_versions")
    remaining = wait_for(
        lambda: _count(db_conn, "page_versions", page["id"], 20),
        what="page_versions trimmed to 20",
    )
    assert remaining == 20


def test_delete_unuploaded_file_asset(db_conn, broker_url):
    # NOTE: FileAsset.objects is a SoftDeletionQuerySet, so .delete()
    # stamps deleted_at instead of removing the row — unlike the
    # cleanup_task deletes, which go through all_objects for hard
    # deletes. Pin the soft delete.
    row = insert_row(
        db_conn,
        "file_assets",
        {
            "asset": "contract/stale.bin",
            "is_uploaded": False,
            "is_deleted": False,
            "is_archived": False,
            "size": 0,
            "attributes": {},
            "created_at": _old(),
        },
    )
    celery_wire.publish(f"{M}.file_asset_task.delete_unuploaded_file_asset")
    assert wait_for(
        lambda: _soft_deleted(db_conn, "file_assets", row["id"]),
        what="stale unuploaded file_assets row soft-deleted",
    )


def test_delete_old_s3_link_clears_expired_url(db_conn, broker_url):
    """Expired exporter URL is cleared without touching storage (empty key)."""
    owner = seed_helpers.user(db_conn, "contract-exporter-owner")
    workspace = seed_helpers.workspace(db_conn, "contractexporter", owner["id"])
    row = insert_row(
        db_conn,
        "exporters",
        {
            "workspace_id": str(workspace["id"]),
            "type": "issue_exports",
            "provider": "csv",
            "status": "completed",
            "reason": "",
            "url": "https://example.com/expired.zip",
            "key": "",
            "token": f"contract-{uuid.uuid4().hex}",
            "initiated_by_id": str(owner["id"]),
            "created_at": _old(days=9),
        },
    )
    celery_wire.publish(f"{M}.exporter_expired_task.delete_old_s3_link")
    cleared = wait_for(
        lambda: _column_is_null(db_conn, "exporters", row["id"], "url"),
        what="expired exporter url cleared",
    )
    assert cleared


def test_hard_delete_removes_tombstoned_rows(db_conn, broker_url):
    owner = seed_helpers.user(db_conn, "contract-tomb-owner")
    workspace = seed_helpers.workspace(db_conn, "contracttomb", owner["id"])
    row = insert_row(
        db_conn,
        "webhook_logs",
        {
            "workspace_id": str(workspace["id"]),
            "webhook": str(uuid.uuid4()),
            "event_type": "issue",
            "retry_count": 0,
            "created_at": _old(),
            "deleted_at": _old(),
        },
    )
    celery_wire.publish(f"{M}.deletion_task.hard_delete")
    wait_for(
        lambda: _missing(db_conn, "webhook_logs", row["id"]),
        what="tombstoned webhook_logs row hard-deleted",
    )


def test_eta_delayed_execution(db_conn, broker_url):
    """ETA parity through the real worker: countdown delays the delete."""
    row = insert_row(
        db_conn,
        "api_activity_logs",
        {
            "token_identifier": "contract-eta",
            "path": "/contract/eta",
            "method": "GET",
            "response_code": 200,
            "created_at": _old(),
        },
    )
    celery_wire.publish(f"{M}.cleanup_task.delete_api_logs", countdown=5)
    assert not _missing(db_conn, "api_activity_logs", row["id"]), (
        "countdown job executed early — ETA parity broken"
    )
    wait_for(
        lambda: _missing(db_conn, "api_activity_logs", row["id"]),
        what="countdown-delayed delete executed",
    )


def test_cleanup_redelivery_deletes_nothing_twice(db_conn, broker_url):
    """Redelivery: a second run over an empty eligible set changes nothing."""
    before = snapshot(db_conn, ["api_activity_logs"])
    celery_wire.publish(f"{M}.cleanup_task.delete_api_logs")
    broker_probe.wait_for_queue_drain(what="first cleanup consumed")
    celery_wire.publish(f"{M}.cleanup_task.delete_api_logs")
    broker_probe.wait_for_queue_drain(what="redelivered cleanup consumed")
    from _harness.db import diff as _diff

    after = snapshot(db_conn, ["api_activity_logs"])
    assert _diff(before, after) == {}


def test_storage_aware_tasks_consumed_without_crash(db_conn, broker_url):
    """Execution parity for S3/live-server-dependent tasks: consumed + acked."""
    cases = [
        (f"{M}.storage_metadata_task.get_asset_object_metadata", [str(uuid.uuid4())], {}),
        (
            f"{M}.copy_s3_object.copy_s3_objects_of_description_and_assets",
            ["ISSUE", str(uuid.uuid4()), str(uuid.uuid4()), "ws", str(uuid.uuid4())],
            {},
        ),
        (f"{M}.workspace_seed_task.workspace_seed", [str(uuid.uuid4())], {}),
        (
            f"{M}.issue_version_sync.schedule_issue_version",
            [],
            {"batch_size": 1, "countdown": 300},
        ),
        (
            f"{M}.issue_description_version_sync.schedule_issue_description_version",
            [],
            {"batch_size": 1, "countdown": 300},
        ),
    ]
    baseline = broker_probe.queue_depth()
    for task_name, args, kwargs in cases:
        celery_wire.publish(task_name, args=args, kwargs=kwargs)
    broker_probe.wait_for_queue_drain(
        baseline=baseline, what="storage-aware D-09 tasks consumed"
    )


def _soft_deleted(conn, table: str, pk) -> bool:
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(f'SELECT deleted_at FROM "{table}" WHERE id = %s::uuid', (str(pk),))
        row = cur.fetchone()
        return row is not None and row["deleted_at"] is not None


def _missing(conn, table: str, pk) -> bool:
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(f'SELECT 1 FROM "{table}" WHERE id = %s::uuid', (str(pk),))
        return cur.fetchone() is None


def _count(conn, table: str, page_id, expected: int):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            f'SELECT count(*) AS n FROM "{table}" WHERE page_id = %s::uuid',
            (str(page_id),),
        )
        n = cur.fetchone()["n"]
        return n if n == expected else None


def _column_is_null(conn, table: str, pk, column: str) -> bool:
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            f'SELECT {column} FROM "{table}" WHERE id = %s::uuid', (str(pk),)
        )
        row = cur.fetchone()
        return row is not None and row[column] is None


def _workspace_total(conn, table: str, workspace_id) -> int:
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            f'SELECT count(*) AS n FROM "{table}" WHERE workspace_id = %s::uuid',
            (str(workspace_id),),
        )
        return cur.fetchone()["n"]


def test_create_dummy_data_before_after(db_conn, broker_url):
    """DB before/after for `create_dummy_data` across every count param.

    Django source: `pi_dash/bgtasks/dummy_data_task.py:488-553`.
    Counts: issue_count=4, cycle_count=2, module_count=2, pages_count=4,
    intake_issue_count=2. Deterministic deltas are asserted exactly;
    sampled link tables (unseeded `random`) are asserted within their
    draw bounds, and the `create_issue_parent` no-op bug is pinned at
    zero persisted parent links.
    """
    owner = seed_helpers.user(db_conn, "contract-dummy-owner")
    workspace = seed_helpers.workspace(db_conn, "contractdummy", owner["id"])
    member_a = seed_helpers.user(db_conn, "contract-dummy-a")
    member_b = seed_helpers.user(db_conn, "contract-dummy-b")
    members = [member_a["email"], member_b["email"]]
    wid = workspace["id"]

    tables = [
        "projects",
        "project_members",
        "states",
        "labels",
        "cycles",
        "modules",
        "pages",
        "project_pages",
        "page_labels",
        "issues",
        "issue_sequences",
        "issue_activities",
        "intakes",
        "intake_issues",
        "issue_assignees",
        "issue_labels",
        "cycle_issues",
        "module_issues",
    ]
    before = {table: _workspace_total(db_conn, table, wid) for table in tables}

    celery_wire.publish(
        f"{M}.dummy_data_task.create_dummy_data",
        args=[workspace["slug"], owner["email"], members, 4, 2, 2, 4, 2],
    )

    def _finished():
        done = (
            _workspace_total(db_conn, "issues", wid) - before["issues"] == 6
            and _workspace_total(db_conn, "issue_activities", wid) - before["issue_activities"] == 6
            and _workspace_total(db_conn, "intake_issues", wid) - before["intake_issues"] == 2
            and _workspace_total(db_conn, "cycle_issues", wid) - before["cycle_issues"] == 2
        )
        return done or None

    wait_for(_finished, what="dummy data run finished")
    after = {table: _workspace_total(db_conn, table, wid) for table in tables}
    delta = {table: after[table] - before[table] for table in tables}

    assert delta["projects"] == 1
    assert delta["project_members"] == 3  # creator + 2 member emails
    assert delta["states"] == 5
    # 50 attempts with ignore_conflicts: seeded Faker (seed 0, color_name +
    # hex_color interleaved) yields 40 distinct (project, name) pairs, so 10
    # conflict and 40 rows persist (dummy_data_task.py:126-143, Label Meta
    # unique_project_name_when_not_deleted).
    assert delta["labels"] == 40
    assert delta["cycles"] == 3  # off-by-one: cycle_count + 1
    assert delta["modules"] == 2
    assert delta["pages"] == 4
    assert delta["project_pages"] == 4
    assert delta["issues"] == 6  # 4 + 2 intake
    assert delta["issue_sequences"] == 6
    assert delta["issue_activities"] == 6
    assert delta["intakes"] == 1
    assert delta["intake_issues"] == 2
    assert delta["cycle_issues"] == 2  # int(issue_count / 2), one link each

    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            'SELECT id FROM "projects" WHERE workspace_id = %s::uuid ORDER BY created_at DESC LIMIT 1',
            (str(wid),),
        )
        pid = cur.fetchone()["id"]
        # create_issue_parent persists nothing (ported bug): no parent links.
        cur.execute(
            'SELECT count(*) AS n FROM "issues" WHERE project_id = %s::uuid AND parent_id IS NOT NULL',
            (str(pid),),
        )
        assert cur.fetchone()["n"] == 0
        # Fresh project: sequence_ids run 1..6 across both issue batches.
        cur.execute(
            'SELECT sequence_id FROM "issues" WHERE project_id = %s::uuid ORDER BY sequence_id',
            (str(pid),),
        )
        assert [row["sequence_id"] for row in cur.fetchall()] == [1, 2, 3, 4, 5, 6]
        cur.execute(
            'SELECT identifier FROM "projects" WHERE id = %s::uuid',
            (str(pid),),
        )
        identifier = cur.fetchone()["identifier"]
        assert identifier == identifier.upper()
        assert 2 <= len(identifier) <= 12
        cur.execute(
            'SELECT status, snoozed_till FROM "intake_issues" WHERE project_id = %s::uuid',
            (str(pid),),
        )
        for row in cur.fetchall():
            assert row["status"] in (-2, -1, 0, 1, 2)
            assert (row["snoozed_till"] is not None) == (row["status"] == 0)

    # Sampled links stay within their draw bounds (3 assignees in pool,
    # 40 persisted labels, randint(0, 5) multi-links over 6 issues).
    assert 0 <= delta["page_labels"] <= 2 * 49
    assert 0 <= delta["issue_assignees"] <= 2 * 2
    assert 0 <= delta["issue_labels"] <= 6 * 5
    assert 0 <= delta["module_issues"] <= 6 * 5
