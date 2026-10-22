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

import re
import uuid
from datetime import datetime, timedelta, timezone
from html.parser import HTMLParser
from io import StringIO

import pytest
from psycopg.rows import dict_row

from _harness import broker_probe, celery_wire, taskspec
from _harness import s3 as s3_helpers
from _harness import seed as seed_helpers
from _harness.db import insert_row, snapshot, wait_for_condition as wait_for
from _harness.soft_delete_entries import ENTRY_CASES, seed_draft_tree

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
    # Non-NULL ip_address: the column is Postgres inet and every real row
    # has one (PIDASHCONV-1289). A NULL here would skip the inet decode path.
    row = insert_row(
        db_conn,
        "api_activity_logs",
        {
            "token_identifier": "contract-probe",
            "path": "/contract/old",
            "method": "GET",
            "response_code": 200,
            "ip_address": "10.0.0.1",
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


def test_delete_old_s3_link_deletes_truthy_key_object(db_conn, broker_url):
    """Expired exporter with a truthy key: the S3 object is deleted (live
    endpoint) AND the URL clears (PIDASHCONV-858)."""
    bucket = s3_helpers.contract_bucket()
    s3_helpers.s3_setup_bucket(bucket)
    key = f"contract-expiry-{uuid.uuid4().hex}.zip"
    s3_helpers.s3_put_key(bucket, key, b"contract-expiry-probe")
    assert s3_helpers.s3_key_exists(bucket, key)
    owner = seed_helpers.user(db_conn, "contract-exporter-s3-owner")
    workspace = seed_helpers.workspace(db_conn, "contractexporters3", owner["id"])
    row = insert_row(
        db_conn,
        "exporters",
        {
            "workspace_id": str(workspace["id"]),
            "type": "issue_exports",
            "provider": "csv",
            "status": "completed",
            "reason": "",
            "url": "https://example.com/expired-s3.zip",
            "key": key,
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
    # The delete precedes the clear per row, so the object is already
    # gone; poll anyway so a missed delete fails loudly, not flakes.
    assert wait_for(
        lambda: not s3_helpers.s3_key_exists(bucket, key),
        what="expired exporter S3 object deleted",
    )


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


def _tombstone(conn, table: str, pk) -> None:
    with conn.cursor() as cur:
        cur.execute(
            f'UPDATE "{table}" SET deleted_at = %s WHERE id = %s::uuid',
            (_old(), str(pk)),
        )


def _seed_loop_tree(db_conn, tag: str):
    """Loop job + target + preference (all live); caller tombstones."""
    owner = seed_helpers.user(db_conn, f"contract-looptomb-owner-{tag}")
    workspace = seed_helpers.workspace(db_conn, f"contractlooptomb{tag}", owner["id"])
    member = seed_helpers.user(db_conn, f"contract-looptomb-member-{tag}")
    job = insert_row(
        db_conn,
        "loop_jobs",
        {
            "slug": f"contract-looptomb-{tag}",
            "name": "oracle job",
            "public_name": "Oracle Job",
            "public_description": "seeded",
            "prompt": "Do the thing.",
            "min_role": 15,
            "enabled": True,
            "is_builtin": False,
            "dtstart": _old(days=1),
            "rrule": "FREQ=DAILY;BYHOUR=3;BYMINUTE=0",
            "tzid": "UTC",
        },
    )
    target = insert_row(
        db_conn,
        "loop_targets",
        {
            "job_id": str(job["id"]),
            "workspace_id": str(workspace["id"]),
            "user_id": str(member["id"]),
            "last_skip_reason": "",
        },
    )
    pref = insert_row(
        db_conn,
        "loop_user_preferences",
        {
            "user_id": str(member["id"]),
            "job_id": str(job["id"]),
            "enabled": False,
        },
    )
    return job, target, pref


def test_hard_delete_removes_tombstoned_subtree(db_conn, broker_url):
    """PIDASHCONV-928 oracle: the Collector removes a tombstoned subtree.

    ``loop_jobs`` sorts before ``loop_targets`` alphabetically, so this is
    the parent-first pair the Rust sweep used to fail on (FK violation).
    Django's per-model ``.delete()`` runs the ORM Collector, which deletes
    CASCADE children along with the parent inside one transaction.
    """
    tag = uuid.uuid4().hex[:8]
    job, target, pref = _seed_loop_tree(db_conn, tag)
    for table, row in (
        ("loop_jobs", job),
        ("loop_targets", target),
        ("loop_user_preferences", pref),
    ):
        _tombstone(db_conn, table, row["id"])
    celery_wire.publish(f"{M}.deletion_task.hard_delete")
    for table, row in (
        ("loop_jobs", job),
        ("loop_targets", target),
        ("loop_user_preferences", pref),
    ):
        wait_for(
            lambda t=table, r=row: _missing(db_conn, t, r["id"]),
            what=f"tombstoned {table} row hard-deleted",
        )


def test_hard_delete_cascade_deletes_live_child(db_conn, broker_url):
    """PIDASHCONV-928 oracle: Collector deletes LIVE CASCADE children.

    A tombstoned parent with a live CASCADE child is routine input, not an
    anomaly: the soft-delete walk never stamps triage states, so every
    soft-deleted project with triage rows is exactly this shape. Django
    resolves it by hard-deleting the live child too, and the Rust port
    matches (see the replay mirror).
    """
    tag = uuid.uuid4().hex[:8]
    job, target, _pref = _seed_loop_tree(db_conn, tag)
    _tombstone(db_conn, "loop_jobs", job["id"])
    celery_wire.publish(f"{M}.deletion_task.hard_delete")
    wait_for(
        lambda: _missing(db_conn, "loop_jobs", job["id"]),
        what="tombstoned loop_jobs row hard-deleted",
    )
    wait_for(
        lambda: _missing(db_conn, "loop_targets", target["id"]),
        what="live loop_targets child cascade-deleted by Collector",
    )


def test_hard_delete_mutual_project_state(db_conn, broker_url):
    """PIDASHCONV-928 oracle: Collector cracks the project/state FK cycle.

    ``projects.default_state_id`` and ``states.project_id`` reference each
    other, so no table order can delete tombstoned project+state rows with
    plain per-table DELETEs. Django's single-transaction Collector checks
    the deferred FKs once at commit and removes both.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928cyc{tag}")
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE projects SET default_state_id = %s::uuid WHERE id = %s::uuid",
            (str(chain["state"]["id"]), str(chain["project"]["id"])),
        )
    for table, key in (
        ("projects", "project"),
        ("states", "state"),
        ("issues", "issue"),
    ):
        _tombstone(db_conn, table, chain[key]["id"])
    celery_wire.publish(f"{M}.deletion_task.hard_delete")
    for table, key in (
        ("projects", "project"),
        ("states", "state"),
        ("issues", "issue"),
    ):
        wait_for(
            lambda t=table, k=key: _missing(db_conn, t, chain[k]["id"]),
            what=f"tombstoned {table} row hard-deleted",
        )


def _future(days: int = 30) -> str:
    return (datetime.now(timezone.utc) + timedelta(days=days)).isoformat()


def _seed_activity(db_conn, chain, issue_id=None, comment_id=None):
    return insert_row(
        db_conn,
        "issue_activities",
        {
            "workspace_id": str(chain["workspace"]["id"]),
            "project_id": str(chain["project"]["id"]),
            "issue_id": str(issue_id) if issue_id else None,
            "issue_comment_id": str(comment_id) if comment_id else None,
            "verb": "created",
            "comment": "",
            "attachments": "{}",
        },
    )


def _seed_comment(db_conn, chain, issue_id):
    return insert_row(
        db_conn,
        "issue_comments",
        {
            "workspace_id": str(chain["workspace"]["id"]),
            "project_id": str(chain["project"]["id"]),
            "issue_id": str(issue_id),
            "comment_stripped": "hi",
            "comment_json": {},
            "comment_html": "<p>hi</p>",
            "attachments": "{}",
            "labels": "{}",
            "access": "INTERNAL",
            "speaker_type": "human",
            "speaker_label": "",
        },
    )


def _seed_pod(db_conn, chain, name):
    return insert_row(
        db_conn,
        "pod",
        {
            "workspace_id": str(chain["workspace"]["id"]),
            "project_id": str(chain["project"]["id"]),
            "name": name,
            "description": "",
            "is_default": False,
        },
    )


def _seed_page(db_conn, chain, name):
    return insert_row(
        db_conn,
        "pages",
        {
            "workspace_id": str(chain["workspace"]["id"]),
            "owned_by_id": str(chain["owner"]["id"]),
            "name": name,
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


def _seed_triage_state(db_conn, chain, name):
    return insert_row(
        db_conn,
        "states",
        {
            "project_id": str(chain["project"]["id"]),
            "workspace_id": str(chain["workspace"]["id"]),
            "name": name,
            "description": "",
            "color": "#000000",
            "slug": "",
            "group": "triage",
            "sequence": 100,
            "is_triage": True,
            "default": False,
        },
    )


def _seed_estimate_point(db_conn, chain, tag):
    est = insert_row(
        db_conn,
        "estimates",
        {
            "workspace_id": str(chain["workspace"]["id"]),
            "project_id": str(chain["project"]["id"]),
            "name": f"est-{tag}",
            "description": "",
            "type": "points",
            "last_used": False,
        },
    )
    return insert_row(
        db_conn,
        "estimate_points",
        {
            "workspace_id": str(chain["workspace"]["id"]),
            "project_id": str(chain["project"]["id"]),
            "estimate_id": str(est["id"]),
            "key": 1,
            "description": "",
            "value": "1",
        },
    )


def _seed_git_account_binding(db_conn, chain):
    account = insert_row(
        db_conn,
        "git_provider_accounts",
        {
            "workspace_id": str(chain["workspace"]["id"]),
            "provider": "github",
            "host_url": "https://github.com",
            "auth_type": "pat",
            "external_account_id": "",
            "external_account_login": "",
            "display_name": "oracle 928",
            "capabilities": {},
            "credential_config": {},
            "status": "connected",
            "last_check_error": "",
            "metadata": {},
        },
    )
    tag = uuid.uuid4().hex[:8]
    repo = insert_row(
        db_conn,
        "git_repositories",
        {
            "provider": "github",
            "host_url": "https://github.com",
            "external_id": f"928-{tag}",
            "name": f"r{tag}",
            "namespace": "n",
            "full_name": f"n/r{tag}",
            "web_url": "https://github.com/n/r",
            "clone_url_http": "https://github.com/n/r.git",
            "clone_url_ssh": "git@github.com:n/r.git",
            "default_branch": "main",
            "is_private": False,
            "metadata": {},
        },
    )
    binding = insert_row(
        db_conn,
        "git_repository_bindings",
        {
            "workspace_id": str(chain["workspace"]["id"]),
            "project_id": str(chain["project"]["id"]),
            "repository_id": str(repo["id"]),
            "provider_account_id": str(account["id"]),
            "actor_id": str(chain["owner"]["id"]),
            "is_sync_enabled": False,
            "clone_auth_mode": "public",
            "last_sync_error": "",
            "metadata": {},
        },
    )
    return account, binding


def _run_hard_delete_expecting_failure(db_conn, marker_table, marker_id):
    """Publish hard_delete, prove it ran, and let it finish failing.

    The marker is a due row in an earlier model (proves the task executed
    past at least that pass); the barrier task published behind hard_delete
    proves the worker progressed past it (the contract worker is solo FIFO,
    and the settle sleep covers any other pool). Afterwards the caller
    asserts its rows remain — which is only meaningful once the run is
    provably over, not merely published.
    """
    import time

    barrier = insert_row(
        db_conn,
        "api_activity_logs",
        {
            "token_identifier": f"contract-928barrier-{uuid.uuid4().hex[:8]}",
            "path": "/contract/928barrier",
            "method": "GET",
            "response_code": 200,
            "created_at": _old(),
        },
    )
    celery_wire.publish(f"{M}.deletion_task.hard_delete")
    celery_wire.publish(f"{M}.cleanup_task.delete_api_logs")
    wait_for(
        lambda: _missing(db_conn, marker_table, marker_id),
        what="hard_delete marker consumed (task provably ran)",
    )
    wait_for(
        lambda: _missing(db_conn, "api_activity_logs", barrier["id"]),
        what="barrier task consumed (worker progressed past hard_delete)",
    )
    time.sleep(3)


def test_hard_delete_do_nothing_fails_with_partial_commits(db_conn, broker_url):
    """PIDASHCONV-928 oracle (BUG-DEL-3): a due issue with ANY activities fails.

    ``issue_activities.issue_id`` is ``DO_NOTHING``: the Collector never
    collects through it, and ``issues`` precedes ``issue_activities`` in both
    phases, so the per-model commit check fails with IntegrityError — even
    when the activity is itself due. Earlier models stay committed (the
    marker workspace is gone) while nothing from ``issues`` onward runs
    (the due page remains).
    """
    tag = uuid.uuid4().hex[:8]
    chain_a = seed_helpers.issue_chain(db_conn, f"928dna{tag}")
    chain_b = seed_helpers.issue_chain(db_conn, f"928dnb{tag}")
    activity = _seed_activity(db_conn, chain_a, issue_id=chain_a["issue"]["id"])
    page = _seed_page(db_conn, chain_a, f"928 due page {tag}")
    for table, pk in (
        ("issues", chain_a["issue"]["id"]),
        ("issue_activities", activity["id"]),
        ("workspaces", chain_b["workspace"]["id"]),
        ("pages", page["id"]),
    ):
        _tombstone(db_conn, table, pk)
    _run_hard_delete_expecting_failure(
        db_conn, "workspaces", chain_b["workspace"]["id"]
    )
    try:
        assert not _missing(db_conn, "issues", chain_a["issue"]["id"])
        assert not _missing(db_conn, "issue_activities", activity["id"])
        assert not _missing(db_conn, "pages", page["id"])
    finally:
        with db_conn.cursor() as cur:
            cur.execute(
                'DELETE FROM "issue_activities" WHERE id = %s::uuid',
                (str(activity["id"]),),
            )
            cur.execute(
                'DELETE FROM "issues" WHERE id = %s::uuid',
                (str(chain_a["issue"]["id"]),),
            )
            cur.execute(
                'DELETE FROM "pages" WHERE id = %s::uuid', (str(page["id"]),)
            )


def test_hard_delete_live_do_nothing_child_fails(db_conn, broker_url):
    """PIDASHCONV-928 oracle: tombstoned parent + LIVE ``DO_NOTHING`` child.

    Same enforcement as the due-activity case (the deferred commit check),
    with a live child: the task fails and both rows remain.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928ldn{tag}")
    marker = seed_helpers.issue_chain(db_conn, f"928ldnm{tag}")
    activity = _seed_activity(db_conn, chain, issue_id=chain["issue"]["id"])
    _tombstone(db_conn, "issues", chain["issue"]["id"])
    _tombstone(db_conn, "workspaces", marker["workspace"]["id"])
    _run_hard_delete_expecting_failure(
        db_conn, "workspaces", marker["workspace"]["id"]
    )
    try:
        assert not _missing(db_conn, "issues", chain["issue"]["id"])
        assert not _missing(db_conn, "issue_activities", activity["id"])
    finally:
        with db_conn.cursor() as cur:
            cur.execute(
                'DELETE FROM "issue_activities" WHERE id = %s::uuid',
                (str(activity["id"]),),
            )
            cur.execute(
                'DELETE FROM "issues" WHERE id = %s::uuid',
                (str(chain["issue"]["id"]),),
            )


def test_hard_delete_due_comment_resolves_after_activity(db_conn, broker_url):
    """PIDASHCONV-928 oracle: due comment + due activity succeeds in order.

    ``issue_activities`` precedes ``issue_comments`` in the named phase, so
    the due activity is gone before the comment's pass runs: both go.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928dco{tag}")
    comment = _seed_comment(db_conn, chain, chain["issue"]["id"])
    activity = _seed_activity(db_conn, chain, comment_id=comment["id"])
    _tombstone(db_conn, "issue_comments", comment["id"])
    _tombstone(db_conn, "issue_activities", activity["id"])
    celery_wire.publish(f"{M}.deletion_task.hard_delete")
    wait_for(
        lambda: _missing(db_conn, "issue_comments", comment["id"]),
        what="tombstoned issue_comments row hard-deleted",
    )
    wait_for(
        lambda: _missing(db_conn, "issue_activities", activity["id"]),
        what="tombstoned issue_activities row hard-deleted",
    )
    assert not _missing(db_conn, "issues", chain["issue"]["id"])


def test_hard_delete_protect_fails_loudly(db_conn, broker_url):
    """PIDASHCONV-928 oracle: tombstoned pod + LIVE assigned issue.

    ``issues.assigned_pod_id`` is ``PROTECT``: the Collector raises
    ``ProtectedError`` during collection (before that model's deletes) and
    the task fails with both rows remaining.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928pro{tag}")
    marker = seed_helpers.issue_chain(db_conn, f"928prom{tag}")
    pod = _seed_pod(db_conn, chain, f"928 pod {tag}")
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET assigned_pod_id = %s::uuid WHERE id = %s::uuid",
            (str(pod["id"]), str(chain["issue"]["id"])),
        )
    _tombstone(db_conn, "pod", pod["id"])
    _tombstone(db_conn, "workspaces", marker["workspace"]["id"])
    _run_hard_delete_expecting_failure(
        db_conn, "workspaces", marker["workspace"]["id"]
    )
    try:
        assert not _missing(db_conn, "pod", pod["id"])
        assert not _missing(db_conn, "issues", chain["issue"]["id"])
    finally:
        with db_conn.cursor() as cur:
            cur.execute(
                'DELETE FROM "issues" WHERE id = %s::uuid',
                (str(chain["issue"]["id"]),),
            )
            cur.execute('DELETE FROM "pod" WHERE id = %s::uuid', (str(pod["id"]),))


def test_hard_delete_due_protect_child_resolves_in_order(db_conn, broker_url):
    """PIDASHCONV-928 oracle: due pod + due assigned issue succeeds in order.

    ``issues`` precedes ``pod`` (last) in the sweep, so the due issue is
    gone before the pod's pass runs its ``PROTECT`` check: both go.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928dpr{tag}")
    pod = _seed_pod(db_conn, chain, f"928 pod {tag}")
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET assigned_pod_id = %s::uuid WHERE id = %s::uuid",
            (str(pod["id"]), str(chain["issue"]["id"])),
        )
    _tombstone(db_conn, "pod", pod["id"])
    _tombstone(db_conn, "issues", chain["issue"]["id"])
    celery_wire.publish(f"{M}.deletion_task.hard_delete")
    wait_for(
        lambda: _missing(db_conn, "pod", pod["id"]),
        what="tombstoned pod row hard-deleted",
    )
    wait_for(
        lambda: _missing(db_conn, "issues", chain["issue"]["id"]),
        what="tombstoned issues row hard-deleted",
    )


def test_hard_delete_parent_first_protect_fails(db_conn, broker_url):
    """PIDASHCONV-928 oracle (BUG-DEL-3): due account + due binding fails.

    ``git_repository_bindings.provider_account_id`` is ``PROTECT`` and the
    account precedes the binding in the sweep, so the check fires on the
    still-present (due) binding: the Collector ignores the delete set and
    the task fails with both rows remaining.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928pfp{tag}")
    marker = seed_helpers.issue_chain(db_conn, f"928pfpm{tag}")
    account, binding = _seed_git_account_binding(db_conn, chain)
    _tombstone(db_conn, "git_provider_accounts", account["id"])
    _tombstone(db_conn, "git_repository_bindings", binding["id"])
    _tombstone(db_conn, "workspaces", marker["workspace"]["id"])
    _run_hard_delete_expecting_failure(
        db_conn, "workspaces", marker["workspace"]["id"]
    )
    try:
        assert not _missing(db_conn, "git_provider_accounts", account["id"])
        assert not _missing(db_conn, "git_repository_bindings", binding["id"])
    finally:
        with db_conn.cursor() as cur:
            cur.execute(
                'DELETE FROM "git_repository_bindings" WHERE id = %s::uuid',
                (str(binding["id"]),),
            )
            cur.execute(
                'DELETE FROM "git_provider_accounts" WHERE id = %s::uuid',
                (str(account["id"]),),
            )


def test_hard_delete_project_cascade_deletes_live_triage(db_conn, broker_url):
    """PIDASHCONV-928 oracle: due project + LIVE triage state both go.

    Triage states are never stamped by the soft-delete walk, so this is the
    routine app-reachable shape (and the case that falsified the
    loud-failure design): the Collector cascade-deletes the live state.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928tri{tag}")
    triage = _seed_triage_state(db_conn, chain, f"928 triage {tag}")
    _tombstone(db_conn, "projects", chain["project"]["id"])
    celery_wire.publish(f"{M}.deletion_task.hard_delete")
    wait_for(
        lambda: _missing(db_conn, "projects", chain["project"]["id"]),
        what="tombstoned projects row hard-deleted",
    )
    wait_for(
        lambda: _missing(db_conn, "states", triage["id"]),
        what="live triage states row cascade-deleted by Collector",
    )


def test_hard_delete_set_null_ignores_manager_scoping(db_conn, broker_url):
    """PIDASHCONV-928 oracle: ``SET_NULL`` nulls through manager guards.

    The Collector nulls via the unscoped base manager: a LIVE triage-state
    issue keeps its row but loses its ``estimate_point_id`` — rows the
    soft-delete walk's ``IssueManager``/``StateManager`` scoping would skip.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928snm{tag}")
    triage = _seed_triage_state(db_conn, chain, f"928 triage {tag}")
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET state_id = %s::uuid WHERE id = %s::uuid",
            (str(triage["id"]), str(chain["issue"]["id"])),
        )
    point = _seed_estimate_point(db_conn, chain, f"928snm{tag}")
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET estimate_point_id = %s::uuid WHERE id = %s::uuid",
            (str(point["id"]), str(chain["issue"]["id"])),
        )
    _tombstone(db_conn, "estimate_points", point["id"])
    celery_wire.publish(f"{M}.deletion_task.hard_delete")
    wait_for(
        lambda: _missing(db_conn, "estimate_points", point["id"]),
        what="tombstoned estimate_points row hard-deleted",
    )
    assert not _missing(db_conn, "issues", chain["issue"]["id"])
    assert _column_is_null(db_conn, "issues", chain["issue"]["id"], "estimate_point_id")


def test_hard_delete_due_set_null_child_goes_first(db_conn, broker_url):
    """PIDASHCONV-928 oracle: due issue + due estimate point both go.

    ``issues`` precedes ``estimate_points`` in the named phase, so the due
    issue is gone before the point's pass runs: both go.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928dsn{tag}")
    point = _seed_estimate_point(db_conn, chain, f"928dsn{tag}")
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET estimate_point_id = %s::uuid WHERE id = %s::uuid",
            (str(point["id"]), str(chain["issue"]["id"])),
        )
    _tombstone(db_conn, "estimate_points", point["id"])
    _tombstone(db_conn, "issues", chain["issue"]["id"])
    celery_wire.publish(f"{M}.deletion_task.hard_delete")
    wait_for(
        lambda: _missing(db_conn, "estimate_points", point["id"]),
        what="tombstoned estimate_points row hard-deleted",
    )
    wait_for(
        lambda: _missing(db_conn, "issues", chain["issue"]["id"]),
        what="tombstoned issues row hard-deleted",
    )


def test_hard_delete_set_null_keeps_future_tombstone(db_conn, broker_url):
    """PIDASHCONV-928 oracle: ``SET_NULL`` nulls without a deleted_at scope.

    A not-yet-due (future-dated) tombstoned issue keeps its row and its
    ``deleted_at`` but loses its ``estimate_point_id``.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928sft{tag}")
    triage = _seed_triage_state(db_conn, chain, f"928 triage {tag}")
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET state_id = %s::uuid WHERE id = %s::uuid",
            (str(triage["id"]), str(chain["issue"]["id"])),
        )
    point = _seed_estimate_point(db_conn, chain, f"928sft{tag}")
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET estimate_point_id = %s::uuid WHERE id = %s::uuid",
            (str(point["id"]), str(chain["issue"]["id"])),
        )
    _tombstone(db_conn, "estimate_points", point["id"])
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET deleted_at = %s WHERE id = %s::uuid",
            (_future(), str(chain["issue"]["id"])),
        )
    celery_wire.publish(f"{M}.deletion_task.hard_delete")
    wait_for(
        lambda: _missing(db_conn, "estimate_points", point["id"]),
        what="tombstoned estimate_points row hard-deleted",
    )
    assert not _missing(db_conn, "issues", chain["issue"]["id"])
    assert _column_is_null(db_conn, "issues", chain["issue"]["id"], "estimate_point_id")
    assert _soft_deleted(db_conn, "issues", chain["issue"]["id"])


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


# ---------------------------------------------------------------------------
# PIDASHCONV-1292: the S3-backed asset tasks execute for real.
# ---------------------------------------------------------------------------

COPY_TASK = f"{M}.copy_s3_object.copy_s3_objects_of_description_and_assets"
METADATA_TASK = f"{M}.storage_metadata_task.get_asset_object_metadata"


def _page_html(conn, page_id) -> str:
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute('SELECT description_html FROM "pages" WHERE id = %s::uuid', (str(page_id),))
        return cur.fetchone()["description_html"]


def _asset_row(conn, asset_id):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            'SELECT id, asset, is_uploaded, storage_metadata FROM "file_assets"'
            " WHERE id = %s::uuid AND deleted_at IS NULL",
            (str(asset_id),),
        )
        return cur.fetchone()


def _image_srcs(html: str) -> list:
    return re.findall(r'<image-component\s+src="([^"]+)"', html or "")


def _seed_copy_world(db_conn, tag: str) -> dict:
    """Page + uploaded source asset + real S3 object for the copy task."""
    chain = seed_helpers.issue_chain(db_conn, f"copy1292-{tag}")
    ws_id = str(chain["workspace"]["id"])
    proj_id = str(chain["project"]["id"])
    bucket = s3_helpers.contract_bucket()
    s3_helpers.s3_setup_bucket(bucket)
    body = b"1292-copy-probe-image"
    src_key = f"{ws_id}/copy1292-{tag}.png"
    s3_helpers.s3_put_key(bucket, src_key, body)
    assert s3_helpers.s3_key_exists(bucket, src_key)
    asset = insert_row(
        db_conn,
        "file_assets",
        {
            "asset": src_key,
            "attributes": {"name": f"copy1292-{tag}.png", "type": "png", "size": len(body)},
            "size": float(len(body)),
            "workspace_id": ws_id,
            "project_id": proj_id,
            "created_by_id": str(chain["owner"]["id"]),
            "entity_type": "PAGE_DESCRIPTION",
            "is_uploaded": True,
            "is_deleted": False,
            "is_archived": False,
            "storage_metadata": {},
        },
    )
    seed_html = f'<p>t</p><image-component src="{asset["id"]}"></image-component>'
    page = insert_row(
        db_conn,
        "pages",
        {
            "workspace_id": ws_id,
            "owned_by_id": str(chain["owner"]["id"]),
            "name": f"copy1292-{tag}",
            "description_json": {},
            "description_html": seed_html,
            "access": 0,
            "color": "",
            "is_locked": False,
            "view_props": {},
            "logo_props": {},
            "is_global": False,
            "sort_order": 65535,
        },
    )
    return {
        "bucket": bucket,
        "body": body,
        "src_key": src_key,
        "asset_id": str(asset["id"]),
        "page_id": str(page["id"]),
        "project_id": proj_id,
        "owner_id": str(chain["owner"]["id"]),
        "slug": chain["workspace"]["slug"],
        "seed_html": seed_html,
    }


def test_copy_task_copies_s3_objects_and_rewrites_page(db_conn, broker_url):
    """Copy task: every rewritten src resolves to a copied S3 object.

    Seeds a page whose description holds one uploaded image-component plus
    its real S3 object, publishes
    ``copy_s3_objects_of_description_and_assets``, and asserts the
    duplicate-side contract: after the worker drains, every
    image-component src in the page resolves to a file_assets row whose
    S3 object exists (HEAD 200). The effect wait (not the queue drain)
    is the gate: the worker acks on receipt, before the copy commits.
    """
    tag = uuid.uuid4().hex[:8]
    world = _seed_copy_world(db_conn, tag)
    celery_wire.publish(
        COPY_TASK,
        kwargs={
            "entity_name": "PAGE",
            "entity_identifier": world["page_id"],
            "project_id": world["project_id"],
            "slug": world["slug"],
            "user_id": world["owner_id"],
        },
    )
    assert wait_for(
        lambda: _page_html(db_conn, world["page_id"]) != world["seed_html"],
        what="copy task rewrote the page description",
    )
    srcs = _image_srcs(_page_html(db_conn, world["page_id"]))
    assert srcs, "rewrite removed every image-component"
    assert world["asset_id"] not in srcs, "srcs still point at the source asset"
    for src in srcs:
        row = _asset_row(db_conn, src)
        assert row is not None, f"src {src} resolves to no file_assets row"
        assert row["is_uploaded"] is True, f"duplicate {src} never marked uploaded"
        assert s3_helpers.s3_key_exists(world["bucket"], row["asset"]), (
            f"duplicate {src} points at S3 key {row['asset']} with no object behind it"
        )
    # Copy, not move: the source object still exists.
    assert s3_helpers.s3_key_exists(world["bucket"], world["src_key"])


def test_metadata_task_stores_head_dict(db_conn, broker_url):
    """Metadata task: storage_metadata becomes the S3 head dict, never NULL.

    Seeds an uploaded asset with the model-default ``{}`` plus its real S3
    object, publishes ``get_asset_object_metadata``, and asserts the field
    lands the Django-shaped head dict (ContentType/ContentLength/
    LastModified/ETag/Metadata) instead of SQL NULL.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"meta1292-{tag}")
    ws_id = str(chain["workspace"]["id"])
    bucket = s3_helpers.contract_bucket()
    s3_helpers.s3_setup_bucket(bucket)
    body = b"1292-metadata-probe-image!"
    key = f"{ws_id}/meta1292-{tag}.png"
    s3_helpers.s3_put_key(bucket, key, body)
    assert s3_helpers.s3_key_exists(bucket, key)
    asset = insert_row(
        db_conn,
        "file_assets",
        {
            "asset": key,
            "attributes": {"name": f"meta1292-{tag}.png", "type": "png", "size": len(body)},
            "size": float(len(body)),
            "workspace_id": ws_id,
            "project_id": str(chain["project"]["id"]),
            "created_by_id": str(chain["owner"]["id"]),
            "entity_type": "PAGE_DESCRIPTION",
            "is_uploaded": True,
            "is_deleted": False,
            "is_archived": False,
            "storage_metadata": {},
        },
    )
    asset_id = str(asset["id"])
    celery_wire.publish(METADATA_TASK, kwargs={"asset_id": asset_id})
    assert wait_for(
        lambda: (_asset_row(db_conn, asset_id) or {}).get("storage_metadata") != {},
        what="metadata task saved storage_metadata",
    )
    meta = _asset_row(db_conn, asset_id)["storage_metadata"]
    assert meta is not None, "storage_metadata saved as SQL NULL"
    assert set(meta) == {"ContentType", "ContentLength", "LastModified", "ETag", "Metadata"}
    assert meta["ContentLength"] == len(body)
    assert meta["ContentType"] == "application/octet-stream"
    assert meta["ETag"], "missing ETag"
    assert meta["LastModified"].endswith("+00:00"), meta["LastModified"]


def test_soft_delete_draftissue_cascades(db_conn, broker_url):
    """PIDASHCONV-1293 oracle: ("db", "draftissue") resolves and the walk
    stamps the CASCADE children — assignees, labels, modules, cycles
    (``db/models/draft.py``) plus the ``file_assets.draft_issue_id`` row
    (``db/models/asset.py``) — and the draft itself. The Rust replay pins
    the identical walk; pre-fix the Rust worker failed it as UnknownModel
    with nothing stamped.
    """
    tag = uuid.uuid4().hex[:8]
    pk, extra = seed_draft_tree(db_conn, tag)
    celery_wire.publish(
        f"{M}.deletion_task.soft_delete_related_objects",
        args=["db", "draftissue", str(pk)],
        kwargs={"using": None},
    )
    wait_for(
        lambda: _soft_deleted(db_conn, "draft_issues", pk),
        what="draft_issues row stamped by django worker",
    )
    for table, child_id in extra["children"]:
        assert _soft_deleted(db_conn, table, child_id), (
            f"cascade broken: live {table} row under a stamped draft"
        )


@pytest.mark.parametrize(
    "model_name,table,seeder", ENTRY_CASES, ids=[case[0] for case in ENTRY_CASES]
)
def test_soft_delete_entry_models_ack(
    db_conn, broker_url, model_name, table, seeder
):
    """PIDASHCONV-1293 oracle: every model name the Rust API enqueues
    resolves on Django — the job acks (the self-stamp is the task's last
    write, so a LookupError would leave it NULL) instead of failing. The
    draft case re-pins the CASCADE children; the schedulerbinding case
    pins the ``agent_run.scheduler_binding_id`` SET_NULL.
    """
    tag = uuid.uuid4().hex[:8]
    pk, extra = seeder(db_conn, tag)
    celery_wire.publish(
        f"{M}.deletion_task.soft_delete_related_objects",
        args=["db", model_name, str(pk)],
        kwargs={"using": None},
    )
    wait_for(
        lambda: _soft_deleted(db_conn, table, pk),
        what=f"{table} row stamped by django worker",
    )
    for child_table, child_id in extra.get("children", []):
        assert _soft_deleted(db_conn, child_table, child_id), (
            f"cascade broken: live {child_table} row under stamped {model_name}"
        )
    if "agent_run_id" in extra:
        assert _column_is_null(
            db_conn, "agent_run", extra["agent_run_id"], "scheduler_binding_id"
        ), "SET_NULL broken: agent_run still points at the stamped binding"


def test_sync_description_version_large_workspace(db_conn, broker_url):
    """Backfill over >4369 live issues in one workspace (PIDASHCONV-1294).

    Django source: `pi_dash/bgtasks/issue_description_version_sync.py:40-120`.
    The sync writes one multi-row INSERT per workspace group at the
    default batch_size; with 4,400 live issues the Rust port exceeded
    sqlx's u16 bind-param cap and wrote nothing. `batch_size` is omitted
    so both backends use their default (5000); `offset` starts at the
    pre-seed total so the window covers exactly the new rows.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.bulk_issues(db_conn, f"1294desc-{tag}", 4400)
    wid = chain["workspace"]["id"]
    celery_wire.publish(
        f"{M}.issue_description_version_sync.sync_issue_description_version",
        kwargs={"offset": chain["pre_total"], "countdown": 300},
    )

    def _done():
        with db_conn.cursor(row_factory=dict_row) as cur:
            cur.execute(
                "SELECT COUNT(*) AS n FROM issue_description_versions"
                " WHERE workspace_id = %s::uuid",
                (str(wid),),
            )
            n = cur.fetchone()["n"]
            return n if n == 4400 else None

    wait_for(_done, what="4400 description versions written")
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT COUNT(*) AS n FROM ("
            " SELECT issue_id FROM issue_description_versions"
            " WHERE workspace_id = %s::uuid GROUP BY issue_id HAVING COUNT(*) = 1"
            ") one",
            (str(wid),),
        )
        assert cur.fetchone()["n"] == 4400


class _MLStripper(HTMLParser):
    """Local copy of Django's MLStripper (html_processor.py:9-25)."""

    def __init__(self):
        super().__init__()
        self.reset()
        self.strict = False
        self.convert_charrefs = True
        self.text = StringIO()

    def handle_data(self, d):
        self.text.write(d)

    def get_data(self):
        return self.text.getvalue()


def _ml_strip(html: str) -> str:
    stripper = _MLStripper()
    stripper.feed(html)
    return stripper.get_data()


@pytest.mark.parametrize(
    "html",
    [
        "<p>a &amp; b</p>",
        "<p>1 < 2 and 3 > 1</p>",
        "<p>x&nbsp;y</p>",
    ],
)
def test_track_page_version_description_stripped(db_conn, broker_url, html):
    """track_page_version stores HTMLParser text: entities decoded, bare < kept.

    PIDASHCONV-1296. PageVersion.save recomputes description_stripped with
    the HTMLParser-based MLStripper, so the version carries ``a & b`` (not
    ``a &amp; b``) and keeps a bare ``<`` as text. The oracle worker boots
    with page_version_task in -I (oracle_worker_includes.txt); the stock
    Django worker never registers it (PB-1510).
    """
    tag = uuid.uuid4().hex[:8]
    owner = seed_helpers.user(db_conn, f"contract-trackver-{tag}")
    workspace = seed_helpers.workspace(db_conn, f"contracttrackver{tag}", owner["id"])
    page = insert_row(
        db_conn,
        "pages",
        {
            "workspace_id": str(workspace["id"]),
            "owned_by_id": str(owner["id"]),
            "name": f"track version {tag}",
            "description_json": {},
            "description_html": html,
            "access": 0,
            "color": "",
            "is_locked": False,
            "view_props": {},
            "logo_props": {},
            "is_global": False,
            "sort_order": 65535,
        },
    )
    celery_wire.publish(
        f"{M}.page_version_task.track_page_version",
        args=[str(page["id"]), None, str(owner["id"])],
    )

    def _version_row():
        with db_conn.cursor(row_factory=dict_row) as cur:
            cur.execute(
                'SELECT description_stripped FROM "page_versions"'
                " WHERE page_id = %s::uuid ORDER BY last_saved_at DESC LIMIT 1",
                (str(page["id"]),),
            )
            return cur.fetchone()

    row = wait_for(_version_row, what="page_version stripped by django worker")
    assert row["description_stripped"] == _ml_strip(html)
