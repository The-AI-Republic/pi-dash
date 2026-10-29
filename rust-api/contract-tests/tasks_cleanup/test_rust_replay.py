# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-09 oracle, Rust-side replay (PIDASHCONV-225).

Executable form of the tasks_cleanup oracle against the Rust worker.
The Django oracle (``test_cleanup_tasks.py`` / ``test_workspace_seed.py``)
publishes jobs in Celery protocol v2 to ``CELERY_BROKER_URL`` and probes
the worker with Celery broadcast inspect — only a Celery (Django) worker
can execute or answer those. The Rust worker consumes the Postgres queue
``rust_job_queue`` instead, so this module replays the same oracle by
seeding ``rust_job_queue`` rows carrying the exact Celery v2 payloads
(see ``_harness/rust_queue.py``) and running the identical DB
before/after assertions.

Run with the Django worker (and beat) STOPPED and the Rust worker
running against the same DATABASE_URL::

    export DATABASE_URL=postgresql://...  # your own scratch database
    export HARD_DELETE_AFTER_DAYS=0 UNUPLOADED_ASSET_DELETE_DAYS=0
    export SEED_DIR=$PWD/../apps/api/pi_dash/seeds/data  # from rust-api/;
    # the *data* dir itself: the worker joins SEED_DIR with the filenames
    # directly (Django appends "data" to its own SEED_DIR). Without this the
    # seed run succeeds empty and the seed test times out.
    pidash-api worker &  # Rust worker; Django worker stopped
    pytest -p no:django tasks_cleanup/test_rust_replay.py

What maps where (``celery_wire`` -> ``rust_queue``):

- ``celery_wire.publish(task, args, kwargs, countdown)`` ->
  ``rust_queue.publish(...)`` (same task name, same args/kwargs;
  ``countdown`` becomes ``visible_at``, mirroring ``NewJob::delayed``).
- ``broker_probe.wait_for_queue_drain`` -> ``rust_queue.wait_for_queue_drain``
  (Postgres depth instead of broker depth).
- ``test_worker_registration`` (Celery inspect) ->
  ``test_rust_worker_liveness`` below: a no-op cleanup job must be
  consumed from ``rust_job_queue``, proving a Rust worker (not Django)
  is serving this database.

What is NOT duplicated here: the static parity tests (wire payload,
task options, ETA call sites, beat entries) are backend-independent —
they assert on Django source text and the in-process wire builder, so
they already pass with no worker at all. Only the live worker tests
have a replay form.

Ownership split (mirrors the worker registry):

- Locally-owned tasks assert the SAME row-level DB diffs as the Django
  oracle, seed for seed: cleanup deletes, page-version windowing,
  unuploaded-asset soft delete, expired-exporter URL clear (+ live S3
  object delete for truthy keys, PIDASHCONV-858), hard delete, ETA
  delay, redelivery, workspace_seed before/after. No skipped or
  weakened assertions.
- Export tasks are Rust-owned: ``delete_old_s3_link`` (PIDASHCONV-858),
  ``issue_export_task`` / ``analytic_export_task`` (PIDASHCONV-1106,
  replayed against captures by the ``exports/`` suite).
  ``export_analytics_to_csv_email`` is dead (defined, never referenced)
  and was dropped from the parity inventory by PIDASHCONV-1108, so it
  is no longer published or asserted on.
"""

import json as _json
import re
import uuid
from datetime import datetime, timedelta, timezone
from html.parser import HTMLParser
from io import StringIO

import pytest
from psycopg.rows import dict_row

from _harness import rust_queue, taskspec as _taskspec
from _harness import s3 as s3_helpers
from _harness import seed as seed_helpers
from _harness.db import insert_row, snapshot, wait_for_condition as wait_for
from _harness.soft_delete_entries import ENTRY_CASES, seed_draft_tree

M = "pi_dash.bgtasks"

def test_rust_worker_liveness(db_conn):
    """Rust inspect-equivalent: a no-op cleanup job is consumed from Postgres.

    Replaces ``test_worker_registration`` (Celery broadcast inspect, which
    only a Django worker answers). An empty eligible set means the DB is
    untouched; consumption itself proves the Rust worker owns the queue.
    """
    before = snapshot(db_conn, ["api_activity_logs"])
    rust_queue.publish(f"{M}.cleanup_task.delete_api_logs")
    rust_queue.wait_for_queue_drain(what="rust worker consumed liveness probe")
    from _harness.db import diff as _diff

    after = snapshot(db_conn, ["api_activity_logs"])
    assert _diff(before, after) == {}


def _old(days: int = 60) -> str:
    return (datetime.now(timezone.utc) - timedelta(days=days)).isoformat()


def test_rust_delete_api_logs(db_conn):
    # Non-NULL ip_address: the column is Postgres inet and every real row
    # has one (PIDASHCONV-1289). A NULL here would skip the inet decode path.
    row = insert_row(
        db_conn,
        "api_activity_logs",
        {
            "token_identifier": "rust-replay-probe",
            "path": "/contract/old",
            "method": "GET",
            "response_code": 200,
            "ip_address": "10.0.0.1",
            "created_at": _old(),
        },
    )
    rust_queue.publish(f"{M}.cleanup_task.delete_api_logs")
    wait_for(
        lambda: _missing(db_conn, "api_activity_logs", row["id"]),
        what="aged api_activity_logs row deleted by rust worker",
    )


def test_rust_delete_email_notification_logs(db_conn):
    receiver = seed_helpers.user(db_conn, "rust-replay-expire")
    actor = seed_helpers.user(db_conn, "rust-replay-expire-actor")
    row = seed_helpers.email_log(
        db_conn,
        receiver["id"],
        actor["id"],
        str(uuid.uuid4()),
        sent_at=_old(),
        created_at=_old(),
    )
    rust_queue.publish(f"{M}.cleanup_task.delete_email_notification_logs")
    wait_for(
        lambda: _missing(db_conn, "email_notification_logs", row["id"]),
        what="aged email_notification_logs row deleted by rust worker",
    )


def test_rust_delete_webhook_logs(db_conn):
    owner = seed_helpers.user(db_conn, "rust-replay-weblog-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplayweblog", owner["id"])
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
    rust_queue.publish(f"{M}.cleanup_task.delete_webhook_logs")
    wait_for(
        lambda: _missing(db_conn, "webhook_logs", row["id"]),
        what="aged webhook_logs row deleted by rust worker",
    )


def test_rust_delete_page_versions_keeps_newest_20(db_conn):
    """Windowing parity: only versions beyond the newest 20 per page go."""
    owner = seed_helpers.user(db_conn, "rust-replay-pagever-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplaypagever", owner["id"])
    page = insert_row(
        db_conn,
        "pages",
        {
            "workspace_id": str(workspace["id"]),
            "owned_by_id": str(owner["id"]),
            "name": "rust replay page",
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
    rust_queue.publish(f"{M}.cleanup_task.delete_page_versions")
    remaining = wait_for(
        lambda: _count(db_conn, "page_versions", page["id"], 20),
        what="page_versions trimmed to 20 by rust worker",
    )
    assert remaining == 20


def test_rust_delete_unuploaded_file_asset(db_conn):
    # NOTE: FileAsset.objects is a SoftDeletionQuerySet, so .delete()
    # stamps deleted_at instead of removing the row — unlike the
    # cleanup_task deletes, which go through all_objects for hard
    # deletes. Pin the soft delete. (Same note as the Django oracle.)
    row = insert_row(
        db_conn,
        "file_assets",
        {
            "asset": "rust-replay/stale.bin",
            "is_uploaded": False,
            "is_deleted": False,
            "is_archived": False,
            "size": 0,
            "attributes": {},
            "created_at": _old(),
        },
    )
    rust_queue.publish(f"{M}.file_asset_task.delete_unuploaded_file_asset")
    assert wait_for(
        lambda: _soft_deleted(db_conn, "file_assets", row["id"]),
        what="stale unuploaded file_assets row soft-deleted by rust worker",
    )


def test_rust_delete_old_s3_link_clears_expired_url(db_conn):
    """Rust replay of the empty-key case: URL clears, no S3 call (PIDASHCONV-858)."""
    owner = seed_helpers.user(db_conn, "rust-replay-exporter-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplayexporter", owner["id"])
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
            "token": f"rust-replay-{uuid.uuid4().hex}",
            "initiated_by_id": str(owner["id"]),
            "created_at": _old(days=9),
        },
    )
    rust_queue.publish(f"{M}.exporter_expired_task.delete_old_s3_link")
    assert wait_for(
        lambda: _column_is_null(db_conn, "exporters", row["id"], "url"),
        what="expired exporter url cleared by rust worker",
    )


def test_rust_delete_old_s3_link_deletes_truthy_key_object(db_conn):
    """Rust replay of the truthy-key case: the S3 object is deleted from
    the live endpoint AND the URL clears (PIDASHCONV-858)."""
    bucket = s3_helpers.contract_bucket()
    s3_helpers.s3_setup_bucket(bucket)
    key = f"rust-replay-expiry-{uuid.uuid4().hex}.zip"
    s3_helpers.s3_put_key(bucket, key, b"rust-replay-expiry-probe")
    assert s3_helpers.s3_key_exists(bucket, key)
    owner = seed_helpers.user(db_conn, "rust-replay-exporter-s3-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplayexporters3", owner["id"])
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
            "token": f"rust-replay-{uuid.uuid4().hex}",
            "initiated_by_id": str(owner["id"]),
            "created_at": _old(days=9),
        },
    )
    rust_queue.publish(f"{M}.exporter_expired_task.delete_old_s3_link")
    assert wait_for(
        lambda: _column_is_null(db_conn, "exporters", row["id"], "url"),
        what="expired exporter url cleared by rust worker",
    )
    # The delete precedes the clear per row, so the object is already
    # gone; poll anyway so a missed delete fails loudly, not flakes.
    assert wait_for(
        lambda: not s3_helpers.s3_key_exists(bucket, key),
        what="expired exporter S3 object deleted by rust worker",
    )


def test_rust_hard_delete_removes_tombstoned_rows(db_conn):
    owner = seed_helpers.user(db_conn, "rust-replay-tomb-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplaytomb", owner["id"])
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
    rust_queue.publish(f"{M}.deletion_task.hard_delete")
    wait_for(
        lambda: _missing(db_conn, "webhook_logs", row["id"]),
        what="tombstoned webhook_logs row hard-deleted by rust worker",
    )


def test_rust_soft_delete_loopjob_cascades(db_conn):
    """PIDASHCONV-900: the Rust worker resolves ("db", "loopjob") and runs
    the soft-delete the loop DELETE handler enqueues (PIDASHCONV-888).

    Django parity (`deletion_task.py:18-105`): reverse-FK related managers
    ARE callable (`RelatedManager.__call__` rebuilds with the named
    manager), so the to-many CASCADE relations (`loop_targets`,
    `loop_user_preferences`) stamp every live child and the job row
    itself is stamped last. Without the ENTRY_TABLES row the Rust worker
    failed this job as UnknownModel and parked it after retries; now the
    queue row is acked (deleted) and the database matches Django exactly.
    """
    owner = seed_helpers.user(db_conn, "rust-replay-loopjob-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplayloopjob", owner["id"])
    member = seed_helpers.user(db_conn, "rust-replay-loopjob-member")
    job = insert_row(
        db_conn,
        "loop_jobs",
        {
            "slug": f"rust-replay-loopjob-{uuid.uuid4().hex[:8]}",
            "name": "replay job",
            "public_name": "Replay Job",
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
    celery_id = rust_queue.publish(
        f"{M}.deletion_task.soft_delete_related_objects",
        args=["db", "loopjob", str(job["id"])],
        kwargs={"using": None},
    )
    wait_for(
        lambda: _soft_deleted(db_conn, "loop_jobs", job["id"]),
        what="loop_jobs row stamped by rust worker",
    )
    rust_queue.wait_for_settled(celery_id, what="loopjob soft-delete acked")
    assert _soft_deleted(db_conn, "loop_targets", target["id"]), (
        "cascade parity broken: live loop_targets child not stamped"
    )
    assert _soft_deleted(db_conn, "loop_user_preferences", pref["id"]), (
        "cascade parity broken: live loop_user_preferences child not stamped"
    )


def test_rust_soft_delete_user_nulls_audit_fks(db_conn):
    """PIDASHCONV-900: `SET_NULL` reverse relations null instead of stamping.

    Django dispatches on `relation.on_delete` (`deletion_task.py:44-64`),
    but emits bare `NO ACTION` FK constraints for every behavior, so the
    Rust walk dispatches from the `_meta`-derived map (audit
    `created_by`/`updated_by` columns plus `SET_NULL_FKS`). Deleting a
    user nulls the audit FKs pointing at them. `users` itself carries no
    `deleted_at` (`User` is not a `SoftDeleteModel`), so Django leaves the
    user row live — the nulled FK is the observable effect.
    """
    owner = seed_helpers.user(db_conn, "rust-replay-null-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplaynull", owner["id"])
    target = seed_helpers.user(db_conn, "rust-replay-null-target")
    row = insert_row(
        db_conn,
        "webhook_logs",
        {
            "workspace_id": str(workspace["id"]),
            "webhook": str(uuid.uuid4()),
            "event_type": "issue",
            "retry_count": 0,
            "created_by_id": str(target["id"]),
        },
    )
    celery_id = rust_queue.publish(
        f"{M}.deletion_task.soft_delete_related_objects",
        args=["db", "user", str(target["id"])],
        kwargs={"using": None},
    )
    wait_for(
        lambda: _column_is_null(db_conn, "webhook_logs", row["id"], "created_by_id"),
        what="audit FK nulled by rust worker",
    )
    rust_queue.wait_for_settled(celery_id, what="user soft-delete acked")
    assert not _missing(db_conn, "users", target["id"])
    assert not _soft_deleted(db_conn, "webhook_logs", row["id"]), (
        "SET_NULL parity broken: nulled child stamped"
    )


def _fresh_project(db_conn, tag):
    """Randomized owner → workspace → project chain (re-run safe)."""
    owner = seed_helpers.user(db_conn, f"rust-replay-{tag}-owner")
    workspace = seed_helpers.workspace(db_conn, f"rreplay{tag}", owner["id"])
    project = seed_helpers.project(db_conn, workspace["id"])
    return owner, workspace, project


def _column_value(conn, table: str, pk, column: str):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            f'SELECT {column} FROM "{table}" WHERE id = %s::uuid', (str(pk),)
        )
        return cur.fetchone()[column]


def _soft_deleted_int(conn, table: str, pk: int) -> bool:
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(f'SELECT deleted_at FROM "{table}" WHERE id = %s', (pk,))
        row = cur.fetchone()
        return row is not None and row["deleted_at"] is not None


def test_rust_soft_delete_project_preserves_triage_subtree(db_conn):
    """PIDASHCONV-900 review defect 1 (CASCADE side): the to-many fetch is
    the `(manager="objects").all()` queryset. `State.objects` is
    `StateManager`, so triage states are never stamped or recursed into —
    but `Issue.objects` is the inherited live-only manager (`IssueManager`
    lives on `issue_objects`, the *default* manager, which only the SET_NULL
    path inherits), so a triage-state issue IS stamped through
    `issues.project_id` CASCADE. A live Django run confirms both halves.
    """
    _owner, workspace, project = _fresh_project(db_conn, "d1casc")
    live = seed_helpers.state(
        db_conn, project["id"], workspace["id"], group="backlog"
    )
    triage = seed_helpers.state(
        db_conn,
        project["id"],
        workspace["id"],
        name="Triage",
        group="triage",
        is_triage=True,
    )
    live_issue = insert_row(
        db_conn,
        "issues",
        {
            "workspace_id": str(workspace["id"]),
            "project_id": str(project["id"]),
            "state_id": str(live["id"]),
            "name": "live issue",
            "description_json": {},
            "description_html": "<p></p>",
            "priority": "none",
            "complexity_score": 0,
            "sequence_id": 1,
            "is_draft": False,
            "sort_order": 65535,
            "git_work_branch": "",
            "workpad": "",
        },
    )
    triage_issue = insert_row(
        db_conn,
        "issues",
        {
            "workspace_id": str(workspace["id"]),
            "project_id": str(project["id"]),
            "state_id": str(triage["id"]),
            "name": "triage issue",
            "description_json": {},
            "description_html": "<p></p>",
            "priority": "none",
            "complexity_score": 0,
            "sequence_id": 2,
            "is_draft": False,
            "sort_order": 65536,
            "git_work_branch": "",
            "workpad": "",
        },
    )
    celery_id = rust_queue.publish(
        f"{M}.deletion_task.soft_delete_related_objects",
        args=["db", "project", str(project["id"])],
        kwargs={"using": None},
    )
    wait_for(
        lambda: _soft_deleted(db_conn, "projects", project["id"]),
        what="project row stamped by rust worker",
    )
    rust_queue.wait_for_settled(celery_id, what="project soft-delete acked")
    assert _soft_deleted(db_conn, "states", live["id"]), "live state not stamped"
    assert _soft_deleted(db_conn, "issues", live_issue["id"]), "live issue not stamped"
    assert not _soft_deleted(db_conn, "states", triage["id"]), (
        "StateManager parity broken: triage state stamped"
    )
    # NOT live: Issue.objects is live-only, so Django stamps triage-state
    # issues through the project CASCADE (verified by the parity probe).
    assert _soft_deleted(db_conn, "issues", triage_issue["id"]), (
        "objects-manager parity broken: triage-state issue not stamped"
    )


def test_rust_soft_delete_user_preserves_triage_audit_fks(db_conn):
    """PIDASHCONV-900 review defect 1 (SET_NULL side): the default reverse
    manager inherits the child default manager, so nulling audit FKs into
    `states` skips triage rows. Deleting a user nulls a live state's
    `created_by_id` but leaves a triage state's untouched.

    Entry asymmetry, documented: Django's task itself raises
    `AttributeError` on `("db", "user")` (`User` has no `all_objects`),
    while the Rust worker deliberately walks it (nulling audit FKs,
    leaving the user row live). The nulling dispatch asserted here is
    queryset-faithful — the `state_created_by` reverse manager excludes
    triage — as the parity probe confirms.
    """
    _owner, workspace, project = _fresh_project(db_conn, "d1null")
    target = seed_helpers.user(db_conn, "rust-replay-d1null-target")
    live = seed_helpers.state(
        db_conn,
        project["id"],
        workspace["id"],
        group="backlog",
        created_by_id=str(target["id"]),
    )
    triage = seed_helpers.state(
        db_conn,
        project["id"],
        workspace["id"],
        name="Triage",
        group="triage",
        is_triage=True,
        created_by_id=str(target["id"]),
    )
    celery_id = rust_queue.publish(
        f"{M}.deletion_task.soft_delete_related_objects",
        args=["db", "user", str(target["id"])],
        kwargs={"using": None},
    )
    wait_for(
        lambda: _column_is_null(db_conn, "states", live["id"], "created_by_id"),
        what="live state audit FK nulled by rust worker",
    )
    rust_queue.wait_for_settled(celery_id, what="user soft-delete acked")
    assert _column_value(db_conn, "states", triage["id"], "created_by_id") == target["id"], (
        "StateManager parity broken: triage state audit FK nulled"
    )


def test_rust_soft_delete_user_keeps_hidden_fks(db_conn):
    """PIDASHCONV-900 review defect 2: `related_name='+'` relations are
    invisible to Python's `get_fields()`, so the walk never nulls them.
    Deleting a user nulls a visible audit FK (control) but leaves the
    hidden `issue_agent_ticker.pending_entry_actor_id` intact.

    Same `("db", "user")` entry asymmetry as the triage-audit test
    above: Django's task raises on user entry, the Rust worker walks
    it. The skip asserted here is `_meta`-faithful — the relation is
    absent from `get_fields()` — as the parity probe confirms.
    """
    _owner, workspace, project = _fresh_project(db_conn, "d2hid")
    state = seed_helpers.state(db_conn, project["id"], workspace["id"])
    target = seed_helpers.user(db_conn, "rust-replay-d2hid-target")
    issue = insert_row(
        db_conn,
        "issues",
        {
            "workspace_id": str(workspace["id"]),
            "project_id": str(project["id"]),
            "state_id": str(state["id"]),
            "name": "ticker issue",
            "description_json": {},
            "description_html": "<p></p>",
            "priority": "none",
            "complexity_score": 0,
            "sequence_id": 1,
            "is_draft": False,
            "sort_order": 65535,
            "git_work_branch": "",
            "workpad": "",
        },
    )
    ticker = insert_row(
        db_conn,
        "issue_agent_ticker",
        {
            "issue_id": str(issue["id"]),
            "pending_entry_actor_id": str(target["id"]),
            "used": 0,
            "granted": 0,
            "waited": 0,
            "user_disabled": False,
            "enabled": True,
            "disarm_reason": "",
            "pending_entry": False,
            "pending_entry_free": False,
            "pending_entry_trigger": "",
        },
    )
    control = insert_row(
        db_conn,
        "webhook_logs",
        {
            "workspace_id": str(workspace["id"]),
            "webhook": str(uuid.uuid4()),
            "event_type": "issue",
            "retry_count": 0,
            "created_by_id": str(target["id"]),
        },
    )
    celery_id = rust_queue.publish(
        f"{M}.deletion_task.soft_delete_related_objects",
        args=["db", "user", str(target["id"])],
        kwargs={"using": None},
    )
    wait_for(
        lambda: _column_is_null(db_conn, "webhook_logs", control["id"], "created_by_id"),
        what="visible audit FK nulled by rust worker",
    )
    rust_queue.wait_for_settled(celery_id, what="user soft-delete acked")
    assert _column_value(
        db_conn, "issue_agent_ticker", ticker["id"], "pending_entry_actor_id"
    ) == ticker["pending_entry_actor_id"], (
        "hidden-reverse parity broken: pending_entry_actor_id nulled"
    )
    assert not _soft_deleted(db_conn, "issue_agent_ticker", ticker["id"])


def test_rust_soft_delete_project_stamps_identifier(db_conn):
    """PIDASHCONV-900 review defect 3: `project_identifiers` has a bigint
    PK and `deleted_at`; Python stamps it through the project O2O
    (`project_identifier`) CASCADE. The Rust walk must stamp it too —
    not error-skip the relation on the non-UUID child PK.
    """
    _owner, workspace, project = _fresh_project(db_conn, "d3int")
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT nextval(pg_get_serial_sequence('project_identifiers', 'id')) AS nid"
        )
        ident_id = cur.fetchone()["nid"]
    ident = insert_row(
        db_conn,
        "project_identifiers",
        {
            "id": ident_id,
            "workspace_id": str(workspace["id"]),
            "project_id": str(project["id"]),
            "name": uuid.uuid4().hex[:12].upper(),
        },
    )
    celery_id = rust_queue.publish(
        f"{M}.deletion_task.soft_delete_related_objects",
        args=["db", "project", str(project["id"])],
        kwargs={"using": None},
    )
    wait_for(
        lambda: _soft_deleted(db_conn, "projects", project["id"]),
        what="project row stamped by rust worker",
    )
    rust_queue.wait_for_settled(celery_id, what="project soft-delete acked")
    assert _soft_deleted_int(db_conn, "project_identifiers", ident["id"]), (
        "bigint-PK parity broken: project_identifiers row not stamped"
    )


def test_rust_soft_delete_project_skips_soft_deleted_identifier(db_conn):
    """PIDASHCONV-936 (CASCADE O2O side): a soft-deleted O2O child is
    found by the unscoped read yet left untouched — no re-stamp, no
    null — exactly like Django's `if not deleted_at` skip
    (`deletion_task.py:70-71`).

    Scope note, read before extending: no SET_NULL reverse-O2O exists
    in this schema (`_meta` + live-catalog censuses: all 9 O2Os take
    the CASCADE branch), so no live-worker replay can reach the
    SET_NULL O2O null-the-soft-deleted-row path — that half is pinned
    by the `one_to_one_select` SQL-shape unit test (unscoped by
    construction) plus the Django `_base_manager` probe recorded on
    the issue. This test pins the preserved CASCADE half with the
    real `project_identifier` O2O and guards the fetch refactor: the
    unscoped read must not change CASCADE outcomes.
    """
    _owner, workspace, project = _fresh_project(db_conn, "o2ocas")
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT nextval(pg_get_serial_sequence('project_identifiers', 'id')) AS nid"
        )
        ident_id = cur.fetchone()["nid"]
    ident = insert_row(
        db_conn,
        "project_identifiers",
        {
            "id": ident_id,
            "workspace_id": str(workspace["id"]),
            "project_id": str(project["id"]),
            "name": uuid.uuid4().hex[:12].upper(),
        },
    )
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            'UPDATE "project_identifiers" SET "deleted_at" = %s WHERE "id" = %s',
            (_old(days=1), ident["id"]),
        )
        cur.execute(
            'SELECT deleted_at, project_id FROM "project_identifiers" WHERE "id" = %s',
            (ident["id"],),
        )
        before = cur.fetchone()
    assert before["deleted_at"] is not None
    celery_id = rust_queue.publish(
        f"{M}.deletion_task.soft_delete_related_objects",
        args=["db", "project", str(project["id"])],
        kwargs={"using": None},
    )
    wait_for(
        lambda: _soft_deleted(db_conn, "projects", project["id"]),
        what="project row stamped by rust worker",
    )
    rust_queue.wait_for_settled(celery_id, what="project soft-delete acked")
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            'SELECT deleted_at, project_id FROM "project_identifiers" WHERE "id" = %s',
            (ident["id"],),
        )
        after = cur.fetchone()
    assert after["deleted_at"] == before["deleted_at"], (
        "CASCADE O2O parity broken: soft-deleted child re-stamped"
    )
    assert str(after["project_id"]) == str(project["id"]), (
        "CASCADE O2O parity broken: soft-deleted child FK nulled"
    )
def _tombstone(conn, table: str, pk) -> None:
    with conn.cursor() as cur:
        cur.execute(
            f'UPDATE "{table}" SET deleted_at = %s WHERE id = %s::uuid',
            (_old(), str(pk)),
        )


def _seed_loop_tree(db_conn, tag: str):
    """Loop job + target + preference (all live); caller tombstones."""
    owner = seed_helpers.user(db_conn, f"rust-replay-looptomb-owner-{tag}")
    workspace = seed_helpers.workspace(
        db_conn, f"rustreplaylooptomb{tag}", owner["id"]
    )
    member = seed_helpers.user(db_conn, f"rust-replay-looptomb-member-{tag}")
    job = insert_row(
        db_conn,
        "loop_jobs",
        {
            "slug": f"rust-replay-looptomb-{tag}",
            "name": "replay job",
            "public_name": "Replay Job",
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


def test_rust_hard_delete_removes_tombstoned_subtree(db_conn):
    """PIDASHCONV-928: the Rust Collector removes a tombstoned subtree.

    ``loop_jobs`` sorts before ``loop_targets`` alphabetically, and the
    pre-fix sweep deleted table by table in that order — so a tombstoned
    parent always failed its deferred FK check at the statement commit,
    aborting the whole task. The fix emulates the per-model Collector
    (CASCADE closure collected, one joint commit); the whole tombstoned
    subtree goes in one run.
    """
    tag = uuid.uuid4().hex[:8]
    job, target, pref = _seed_loop_tree(db_conn, tag)
    for table, row in (
        ("loop_jobs", job),
        ("loop_targets", target),
        ("loop_user_preferences", pref),
    ):
        _tombstone(db_conn, table, row["id"])
    rust_queue.publish(f"{M}.deletion_task.hard_delete")
    for table, row in (
        ("loop_jobs", job),
        ("loop_targets", target),
        ("loop_user_preferences", pref),
    ):
        wait_for(
            lambda t=table, r=row: _missing(db_conn, t, r["id"]),
            what=f"tombstoned {table} row hard-deleted by rust worker",
        )


def _queue_failure(conn, celery_id):
    """Parked-failure text, or None while the job has not failed.

    A ``Verdict::Fail`` parks immediately (``status = 'failed'`` with the
    error text; only ``Verdict::Retry`` requeues), so the parked row —
    not the attempts counter — is the failure signal.
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT status, last_error FROM rust_job_queue WHERE celery_id = %s",
            (celery_id,),
        )
        row = cur.fetchone()
        if row is None or row["status"] != "failed":
            return None
        return row["last_error"]


def test_rust_hard_delete_cascade_deletes_live_child(db_conn):
    """PIDASHCONV-928: the Rust Collector deletes LIVE CASCADE children.

    A tombstoned parent with live CASCADE children is routine input (the
    soft-delete walk never stamps triage states), so the emulation collects
    them exactly like Django's Collector (see the
    ``test_hard_delete_cascade_deletes_live_child`` oracle): the whole
    subtree goes in one run. Loud failure is reserved for ``PROTECT`` /
    ``DO_NOTHING`` remnants (see the pins below).
    """
    tag = uuid.uuid4().hex[:8]
    job, target, pref = _seed_loop_tree(db_conn, tag)
    _tombstone(db_conn, "loop_jobs", job["id"])
    rust_queue.publish(f"{M}.deletion_task.hard_delete")
    for table, row in (
        ("loop_jobs", job),
        ("loop_targets", target),
        ("loop_user_preferences", pref),
    ):
        wait_for(
            lambda t=table, r=row: _missing(db_conn, t, r["id"]),
            what=f"loop {table} row hard-deleted by rust worker",
        )


def test_rust_hard_delete_mutual_project_state(db_conn):
    """PIDASHCONV-928: the model transaction cracks the project/state cycle.

    ``projects.default_state_id`` (``SET_NULL``) and ``states.project_id``
    (``CASCADE``) reference each other, so no table order deletes tombstoned
    project+state rows with plain per-table DELETEs. The project model's
    pass collects the state, nulls the ``SET_NULL`` edge, and commits both
    deletes jointly — the deferred FK check sees the consistent set gone.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928rcyc{tag}")
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
    rust_queue.publish(f"{M}.deletion_task.hard_delete")
    for table, key in (
        ("projects", "project"),
        ("states", "state"),
        ("issues", "issue"),
    ):
        wait_for(
            lambda t=table, k=key: _missing(db_conn, t, chain[k]["id"]),
            what=f"tombstoned {table} row hard-deleted by rust worker",
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
            "display_name": "replay 928",
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


def _drop_queue_row(conn, celery_id):
    with conn.cursor() as cur:
        cur.execute(
            "DELETE FROM rust_job_queue WHERE celery_id = %s", (celery_id,)
        )


def test_rust_hard_delete_do_nothing_fails_with_partial_commits(
    db_conn
):
    """PIDASHCONV-928 replay (BUG-DEL-3): due issue + due activity fails.

    Mirrors ``test_hard_delete_do_nothing_fails_with_partial_commits``: the
    ``DO_NOTHING`` edge is never collected, so the issues model's commit
    check fails with the FK violation — the earlier workspace stays
    committed, the later page remains, and the queue row parks as failed.
    Seeds are removed at the end: a leftover due issue would fail every
    later hard_delete run in this suite.
    """
    tag = uuid.uuid4().hex[:8]
    chain_a = seed_helpers.issue_chain(db_conn, f"928rdna{tag}")
    chain_b = seed_helpers.issue_chain(db_conn, f"928rdnb{tag}")
    activity = _seed_activity(db_conn, chain_a, issue_id=chain_a["issue"]["id"])
    page = _seed_page(db_conn, chain_a, f"928 due page {tag}")
    for table, pk in (
        ("issues", chain_a["issue"]["id"]),
        ("issue_activities", activity["id"]),
        ("workspaces", chain_b["workspace"]["id"]),
        ("pages", page["id"]),
    ):
        _tombstone(db_conn, table, pk)
    celery_id = rust_queue.publish(f"{M}.deletion_task.hard_delete")
    try:
        failure = wait_for(
            lambda: _queue_failure(db_conn, celery_id),
            what="rust hard_delete parked as failed on DO_NOTHING remnant",
        )
        assert "foreign key" in (failure or ""), (
            f"expected an FK violation in last_error, got: {failure!r}"
        )
        assert _missing(db_conn, "workspaces", chain_b["workspace"]["id"]), (
            "partial-commit parity broken: earlier model was rolled back"
        )
        assert not _missing(db_conn, "issues", chain_a["issue"]["id"])
        assert not _missing(db_conn, "issue_activities", activity["id"])
        assert not _missing(db_conn, "pages", page["id"])
    finally:
        _drop_queue_row(db_conn, celery_id)
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


def test_rust_hard_delete_live_do_nothing_child_fails(db_conn):
    """PIDASHCONV-928 replay: tombstoned parent + LIVE ``DO_NOTHING`` child.

    Mirrors ``test_hard_delete_live_do_nothing_child_fails``: the deferred
    commit check fails on the live child and both rows remain.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928rldn{tag}")
    activity = _seed_activity(db_conn, chain, issue_id=chain["issue"]["id"])
    _tombstone(db_conn, "issues", chain["issue"]["id"])
    celery_id = rust_queue.publish(f"{M}.deletion_task.hard_delete")
    try:
        failure = wait_for(
            lambda: _queue_failure(db_conn, celery_id),
            what="rust hard_delete parked as failed on live DO_NOTHING child",
        )
        assert "foreign key" in (failure or ""), (
            f"expected an FK violation in last_error, got: {failure!r}"
        )
        assert not _missing(db_conn, "issues", chain["issue"]["id"])
        assert not _missing(db_conn, "issue_activities", activity["id"])
    finally:
        _drop_queue_row(db_conn, celery_id)
        with db_conn.cursor() as cur:
            cur.execute(
                'DELETE FROM "issue_activities" WHERE id = %s::uuid',
                (str(activity["id"]),),
            )
            cur.execute(
                'DELETE FROM "issues" WHERE id = %s::uuid',
                (str(chain["issue"]["id"]),),
            )


def test_rust_hard_delete_due_comment_resolves_after_activity(
    db_conn
):
    """PIDASHCONV-928 replay: due comment + due activity succeeds in order.

    Mirrors ``test_hard_delete_due_comment_resolves_after_activity``: the
    activities model precedes comments, so both go.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928rdco{tag}")
    comment = _seed_comment(db_conn, chain, chain["issue"]["id"])
    activity = _seed_activity(db_conn, chain, comment_id=comment["id"])
    _tombstone(db_conn, "issue_comments", comment["id"])
    _tombstone(db_conn, "issue_activities", activity["id"])
    rust_queue.publish(f"{M}.deletion_task.hard_delete")
    wait_for(
        lambda: _missing(db_conn, "issue_comments", comment["id"]),
        what="tombstoned issue_comments row hard-deleted by rust worker",
    )
    wait_for(
        lambda: _missing(db_conn, "issue_activities", activity["id"]),
        what="tombstoned issue_activities row hard-deleted by rust worker",
    )
    assert not _missing(db_conn, "issues", chain["issue"]["id"])


def test_rust_hard_delete_protect_fails_loudly(db_conn):
    """PIDASHCONV-928 replay: tombstoned pod + LIVE assigned issue.

    Mirrors ``test_hard_delete_protect_fails_loudly``: the ``PROTECT``
    pre-check aborts before any write of the pod model's pass and the queue
    row parks with the protected-key message.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928rpro{tag}")
    pod = _seed_pod(db_conn, chain, f"928 pod {tag}")
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET assigned_pod_id = %s::uuid WHERE id = %s::uuid",
            (str(pod["id"]), str(chain["issue"]["id"])),
        )
    _tombstone(db_conn, "pod", pod["id"])
    celery_id = rust_queue.publish(f"{M}.deletion_task.hard_delete")
    try:
        failure = wait_for(
            lambda: _queue_failure(db_conn, celery_id),
            what="rust hard_delete parked as failed on PROTECT remnant",
        )
        assert "protected foreign key" in (failure or ""), (
            f"expected a protected-key message in last_error, got: {failure!r}"
        )
        assert not _missing(db_conn, "pod", pod["id"])
        assert not _missing(db_conn, "issues", chain["issue"]["id"])
    finally:
        _drop_queue_row(db_conn, celery_id)
        with db_conn.cursor() as cur:
            cur.execute(
                'DELETE FROM "issues" WHERE id = %s::uuid',
                (str(chain["issue"]["id"]),),
            )
            cur.execute('DELETE FROM "pod" WHERE id = %s::uuid', (str(pod["id"]),))


def test_rust_hard_delete_due_protect_child_resolves_in_order(
    db_conn
):
    """PIDASHCONV-928 replay: due pod + due assigned issue succeeds in order.

    Mirrors ``test_hard_delete_due_protect_child_resolves_in_order``: the
    issues model precedes ``pod`` (last), so both go.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928rdpr{tag}")
    pod = _seed_pod(db_conn, chain, f"928 pod {tag}")
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET assigned_pod_id = %s::uuid WHERE id = %s::uuid",
            (str(pod["id"]), str(chain["issue"]["id"])),
        )
    _tombstone(db_conn, "pod", pod["id"])
    _tombstone(db_conn, "issues", chain["issue"]["id"])
    rust_queue.publish(f"{M}.deletion_task.hard_delete")
    wait_for(
        lambda: _missing(db_conn, "pod", pod["id"]),
        what="tombstoned pod row hard-deleted by rust worker",
    )
    wait_for(
        lambda: _missing(db_conn, "issues", chain["issue"]["id"]),
        what="tombstoned issues row hard-deleted by rust worker",
    )


def test_rust_hard_delete_parent_first_protect_fails(db_conn):
    """PIDASHCONV-928 replay (BUG-DEL-3): due account + due binding fails.

    Mirrors ``test_hard_delete_parent_first_protect_fails``: the account
    precedes the binding in the sweep, so the ``PROTECT`` check fires on
    the still-present due binding and both rows remain.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928rpfp{tag}")
    account, binding = _seed_git_account_binding(db_conn, chain)
    _tombstone(db_conn, "git_provider_accounts", account["id"])
    _tombstone(db_conn, "git_repository_bindings", binding["id"])
    celery_id = rust_queue.publish(f"{M}.deletion_task.hard_delete")
    try:
        failure = wait_for(
            lambda: _queue_failure(db_conn, celery_id),
            what="rust hard_delete parked as failed on parent-first PROTECT",
        )
        assert "protected foreign key" in (failure or ""), (
            f"expected a protected-key message in last_error, got: {failure!r}"
        )
        assert not _missing(db_conn, "git_provider_accounts", account["id"])
        assert not _missing(db_conn, "git_repository_bindings", binding["id"])
    finally:
        _drop_queue_row(db_conn, celery_id)
        with db_conn.cursor() as cur:
            cur.execute(
                'DELETE FROM "git_repository_bindings" WHERE id = %s::uuid',
                (str(binding["id"]),),
            )
            cur.execute(
                'DELETE FROM "git_provider_accounts" WHERE id = %s::uuid',
                (str(account["id"]),),
            )


def test_rust_hard_delete_project_cascade_deletes_live_triage(
    db_conn
):
    """PIDASHCONV-928 replay: due project + LIVE triage state both go.

    Mirrors ``test_hard_delete_project_cascade_deletes_live_triage``: the
    routine app-reachable shape (triage states are never stamped) that the
    old loud-failure design could not survive.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928rtri{tag}")
    triage = _seed_triage_state(db_conn, chain, f"928 triage {tag}")
    _tombstone(db_conn, "projects", chain["project"]["id"])
    rust_queue.publish(f"{M}.deletion_task.hard_delete")
    wait_for(
        lambda: _missing(db_conn, "projects", chain["project"]["id"]),
        what="tombstoned projects row hard-deleted by rust worker",
    )
    wait_for(
        lambda: _missing(db_conn, "states", triage["id"]),
        what="live triage states row cascade-deleted by rust worker",
    )


def test_rust_hard_delete_set_null_ignores_manager_scoping(
    db_conn
):
    """PIDASHCONV-928 replay: ``SET_NULL`` nulls through manager guards.

    Mirrors ``test_hard_delete_set_null_ignores_manager_scoping``: the
    live triage-state issue keeps its row but loses its
    ``estimate_point_id``.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928rsnm{tag}")
    triage = _seed_triage_state(db_conn, chain, f"928 triage {tag}")
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET state_id = %s::uuid WHERE id = %s::uuid",
            (str(triage["id"]), str(chain["issue"]["id"])),
        )
    point = _seed_estimate_point(db_conn, chain, f"928rsnm{tag}")
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET estimate_point_id = %s::uuid WHERE id = %s::uuid",
            (str(point["id"]), str(chain["issue"]["id"])),
        )
    _tombstone(db_conn, "estimate_points", point["id"])
    rust_queue.publish(f"{M}.deletion_task.hard_delete")
    wait_for(
        lambda: _missing(db_conn, "estimate_points", point["id"]),
        what="tombstoned estimate_points row hard-deleted by rust worker",
    )
    assert not _missing(db_conn, "issues", chain["issue"]["id"])
    assert _column_is_null(
        db_conn, "issues", chain["issue"]["id"], "estimate_point_id"
    )


def test_rust_hard_delete_due_set_null_child_goes_first(db_conn):
    """PIDASHCONV-928 replay: due issue + due estimate point both go.

    Mirrors ``test_hard_delete_due_set_null_child_goes_first``: the issues
    model precedes estimate points, so both go.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928rdsn{tag}")
    point = _seed_estimate_point(db_conn, chain, f"928rdsn{tag}")
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET estimate_point_id = %s::uuid WHERE id = %s::uuid",
            (str(point["id"]), str(chain["issue"]["id"])),
        )
    _tombstone(db_conn, "estimate_points", point["id"])
    _tombstone(db_conn, "issues", chain["issue"]["id"])
    rust_queue.publish(f"{M}.deletion_task.hard_delete")
    wait_for(
        lambda: _missing(db_conn, "estimate_points", point["id"]),
        what="tombstoned estimate_points row hard-deleted by rust worker",
    )
    wait_for(
        lambda: _missing(db_conn, "issues", chain["issue"]["id"]),
        what="tombstoned issues row hard-deleted by rust worker",
    )


def test_rust_hard_delete_set_null_keeps_future_tombstone(
    db_conn
):
    """PIDASHCONV-928 replay: ``SET_NULL`` nulls without a deleted_at scope.

    Mirrors ``test_hard_delete_set_null_keeps_future_tombstone``: the
    future-dated tombstoned issue keeps its row and ``deleted_at`` but
    loses its ``estimate_point_id``.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"928rsft{tag}")
    triage = _seed_triage_state(db_conn, chain, f"928 triage {tag}")
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET state_id = %s::uuid WHERE id = %s::uuid",
            (str(triage["id"]), str(chain["issue"]["id"])),
        )
    point = _seed_estimate_point(db_conn, chain, f"928rsft{tag}")
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
    rust_queue.publish(f"{M}.deletion_task.hard_delete")
    wait_for(
        lambda: _missing(db_conn, "estimate_points", point["id"]),
        what="tombstoned estimate_points row hard-deleted by rust worker",
    )
    assert not _missing(db_conn, "issues", chain["issue"]["id"])
    assert _column_is_null(
        db_conn, "issues", chain["issue"]["id"], "estimate_point_id"
    )
    assert _soft_deleted(db_conn, "issues", chain["issue"]["id"])


def test_rust_eta_delayed_execution(db_conn):
    """ETA parity through the Rust worker: countdown delays the delete."""
    row = insert_row(
        db_conn,
        "api_activity_logs",
        {
            "token_identifier": "rust-replay-eta",
            "path": "/contract/eta",
            "method": "GET",
            "response_code": 200,
            "created_at": _old(),
        },
    )
    rust_queue.publish(f"{M}.cleanup_task.delete_api_logs", countdown=5)
    assert not _missing(db_conn, "api_activity_logs", row["id"]), (
        "countdown job executed early — ETA parity broken"
    )
    wait_for(
        lambda: _missing(db_conn, "api_activity_logs", row["id"]),
        what="countdown-delayed delete executed by rust worker",
    )


def test_rust_cleanup_redelivery_deletes_nothing_twice(db_conn):
    """Redelivery: a second run over an empty eligible set changes nothing."""
    before = snapshot(db_conn, ["api_activity_logs"])
    rust_queue.publish(f"{M}.cleanup_task.delete_api_logs")
    rust_queue.wait_for_queue_drain(what="first rust cleanup consumed")
    rust_queue.publish(f"{M}.cleanup_task.delete_api_logs")
    rust_queue.wait_for_queue_drain(what="redelivered rust cleanup consumed")
    from _harness.db import diff as _diff

    after = snapshot(db_conn, ["api_activity_logs"])
    assert _diff(before, after) == {}


def test_rust_storage_aware_tasks_consumed_without_crash(db_conn):
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
    baseline = rust_queue.queue_depth()
    for task_name, args, kwargs in cases:
        rust_queue.publish(task_name, args=args, kwargs=kwargs)
    rust_queue.wait_for_queue_drain(
        baseline=baseline, what="storage-aware D-09 tasks consumed by rust worker"
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


def _column_is_null(conn, table: str, pk, column: str) -> bool:
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            f'SELECT {column} FROM "{table}" WHERE id = %s::uuid', (str(pk),)
        )
        row = cur.fetchone()
        return row is not None and row[column] is None


def _count(conn, table: str, page_id, expected: int):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            f'SELECT count(*) AS n FROM "{table}" WHERE page_id = %s::uuid',
            (str(page_id),),
        )
        n = cur.fetchone()["n"]
        return n if n == expected else None


# --- workspace_seed replay (mirrors test_workspace_seed_before_after) ---
#
# Helpers below duplicate the tiny row readers from
# test_workspace_seed.py (seed-file access + generic count/one/project
# reads); every assertion that follows is identical to the Django oracle,
# only the publish path changes (rust queue instead of the broker).


def _seed_rows(name):
    root = _taskspec.source_root() / "pi_dash" / "seeds" / "data"
    with open(root / name) as fh:
        return _json.load(fh)


def _seed_count(conn, table, where="", params=()):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(f'SELECT count(*) AS n FROM "{table}" {where}', params)
        return cur.fetchone()["n"]


def _seed_one(conn, table, where, params=()):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(f'SELECT * FROM "{table}" {where}', params)
        return cur.fetchone()


def _seed_project_rows(conn, table, project_id):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            f'SELECT * FROM "{table}" WHERE project_id = %s::uuid', (str(project_id),)
        )
        return cur.fetchall()


def test_rust_workspace_seed_before_after(db_conn):
    tag = uuid.uuid4().hex[:8]
    owner = seed_helpers.user(db_conn, f"rust-replay-seed-owner-{tag}")
    other = seed_helpers.user(db_conn, f"rust-replay-seed-other-{tag}")
    workspace = seed_helpers.workspace(
        db_conn,
        f"rustreplayseed{tag}",
        owner["id"],
        name=f"Seed Rust Replay {tag}!",
    )
    seed_helpers.workspace_member(db_conn, workspace["id"], owner["id"], role=5)
    seed_helpers.workspace_member(db_conn, workspace["id"], other["id"], role=10)

    seed_projects = _seed_rows("projects.json")
    seed_states = _seed_rows("states.json")
    seed_labels = _seed_rows("labels.json")
    seed_cycles = _seed_rows("cycles.json")
    seed_modules = _seed_rows("modules.json")
    seed_issues = _seed_rows("issues.json")
    seed_views = _seed_rows("views.json")
    seed_pages = _seed_rows("pages.json")

    expected_labels = sum(len(r.get("labels", [])) for r in seed_issues)
    expected_cycles = sum(1 for r in seed_issues if r.get("cycle_id"))
    expected_modules = sum(len(r.get("module_ids") or []) for r in seed_issues)
    expected_project_pages = sum(
        1
        for r in seed_pages
        if r.get("project_id") and r.get("type") == "PROJECT"
    )

    before_projects = _seed_count(db_conn, "projects")

    seed_celery_id = rust_queue.publish(
        f"{M}.workspace_seed_task.workspace_seed", args=[str(workspace["id"])]
    )

    project = wait_for(
        lambda: _seed_one(
            db_conn,
            "projects",
            "WHERE workspace_id = %s::uuid AND deleted_at IS NULL",
            (str(workspace["id"]),),
        ),
        what="rust seed project created",
    )
    wait_for(
        lambda: (
            _seed_count(
                db_conn,
                "issues",
                "WHERE project_id = %s::uuid",
                (str(project["id"]),),
            )
            == len(seed_issues)
        )
        or None,
        what="rust seed issues created",
    )
    # The seed job creates views/pages after issues: wait for the ack
    # itself before asserting the tail entities, or a loaded box reads
    # zero pages (PIDASHCONV-900 run 3: deterministic on fresh DBs
    # under load, pages land seconds after issues).
    rust_queue.wait_for_settled(seed_celery_id, what="rust seed job acked")

    # Project shape: workspace name, derived identifier, seed payload,
    # bot audit, first-project default, workspace timezone.
    assert _seed_count(db_conn, "projects") == before_projects + 1
    assert project["name"] == workspace["name"]
    expected_identifier = "".join(
        ch for ch in workspace["name"] if ch.isalnum()
    )[:5].upper()
    assert project["identifier"] == expected_identifier
    assert project["description"] == seed_projects[0]["description"]
    assert project["network"] == seed_projects[0]["network"]
    assert project["cover_image"] == seed_projects[0]["cover_image"]
    assert project["logo_props"] == seed_projects[0]["logo_props"]
    assert project["is_default"] is True
    assert project["timezone"] == workspace["timezone"]
    assert project["cycle_view"] is True
    assert project["module_view"] is True
    assert project["issue_views_view"] is True

    bot = _seed_one(
        db_conn,
        "users",
        "WHERE username = %s",
        (f"bot_user_{workspace['id']}",),
    )
    assert bot is not None
    assert bot["is_bot"] is True
    assert bot["bot_type"] == "WORKSPACE_SEED"
    assert bot["email"] == f"bot_user_{workspace['id']}@example.com"
    assert bot["password"].startswith("pbkdf2_sha256$600000$")
    assert bot["is_password_autoset"] is True
    assert project["created_by_id"] == bot["id"]

    bot_membership = _seed_one(
        db_conn,
        "workspace_members",
        "WHERE workspace_id = %s::uuid AND member_id = %s::uuid",
        (str(workspace["id"]), str(bot["id"])),
    )
    assert bot_membership is not None
    assert bot_membership["role"] == 20
    assert bot_membership["company_role"] == ""

    # Fan-out covers owner + other + bot (3 members).
    members = _seed_project_rows(db_conn, "project_members", project["id"])
    assert {str(m["member_id"]) for m in members} == {
        str(owner["id"]),
        str(other["id"]),
        str(bot["id"]),
    }
    roles = {str(m["member_id"]): m["role"] for m in members}
    assert roles[str(owner["id"])] == 5
    assert roles[str(other["id"])] == 10
    assert all(m["created_by_id"] == bot["id"] for m in members)
    props = _seed_project_rows(db_conn, "project_user_properties", project["id"])
    assert len(props) == 3
    assert props[0]["display_filters"]["group_by"] == "state"
    assert props[0]["display_properties"]["customer_request_count"] is True

    # States / labels keep seed values on the first row of each family.
    states = _seed_project_rows(db_conn, "states", project["id"])
    assert len(states) == len(seed_states)
    by_name = {s["name"]: s for s in states}
    assert by_name["Backlog"]["slug"] == "backlog"
    assert by_name["In Progress"]["slug"] == "in-progress"
    assert by_name["Backlog"]["sequence"] == 15000
    assert by_name["Backlog"]["default"] is True
    assert {s["created_by_id"] for s in states} == {bot["id"]}

    labels = _seed_project_rows(db_conn, "labels", project["id"])
    assert len(labels) == len(seed_labels)
    assert {label["name"] for label in labels} == {
        r["name"] for r in seed_labels
    }

    # Cycles: CURRENT covers now, UPCOMING chains off its end.
    cycles = _seed_project_rows(db_conn, "cycles", project["id"])
    assert len(cycles) == len(seed_cycles)
    current = next(
        c for c in cycles if c["start_date"] <= datetime.now(timezone.utc) <= c["end_date"]
    )
    upcoming = next(c for c in cycles if c["id"] != current["id"])
    assert upcoming["start_date"] >= current["end_date"]
    assert all(c["owned_by_id"] == bot["id"] for c in cycles)

    modules = _seed_project_rows(db_conn, "modules", project["id"])
    assert len(modules) == len(seed_modules)
    assert all(m["target_date"] > m["start_date"] for m in modules)

    # Issues: sequences overwrite the seed values (1-based in file order),
    # stripped descriptions derive from the HTML, completed_at stays null
    # (no seed issue sits in a completed state), and every issue owns TWO
    # sequence rows (save auto-row + explicit create, ported as-is).
    issues = _seed_project_rows(db_conn, "issues", project["id"])
    assert len(issues) == len(seed_issues)
    welcome = next(i for i in issues if i["name"].startswith("Welcome to Pi Dash"))
    assert welcome["sequence_id"] == 1
    assert welcome["priority"] == "urgent"
    assert welcome["description_stripped"] is not None
    assert "<p" not in (welcome["description_stripped"] or "")
    assert all(i["completed_at"] is None for i in issues)
    assert all(i["created_by_id"] == bot["id"] for i in issues)
    for issue in issues:
        seqs = _seed_project_rows(db_conn, "issue_sequences", project["id"])
        mine = [s for s in seqs if str(s["issue_id"]) == str(issue["id"])]
        assert len(mine) == 2, f"issue {issue['id']} must own two sequence rows"
    assert _seed_count(
        db_conn, "issue_sequences", "WHERE project_id = %s::uuid", (str(project["id"]),)
    ) == 2 * len(seed_issues)
    assert (
        _seed_count(
            db_conn,
            "issue_activities",
            "WHERE project_id = %s::uuid AND verb = 'created'",
            (str(project["id"]),),
        )
        == len(seed_issues)
    )
    assert (
        _seed_count(
            db_conn, "issue_labels", "WHERE project_id = %s::uuid", (str(project["id"]),)
        )
        == expected_labels
    )
    assert (
        _seed_count(
            db_conn, "cycle_issues", "WHERE project_id = %s::uuid", (str(project["id"]),)
        )
        == expected_cycles
    )
    assert (
        _seed_count(
            db_conn,
            "module_issues",
            "WHERE project_id = %s::uuid",
            (str(project["id"]),),
        )
        == expected_modules
    )

    # Link-row audit wipe (PIDASHCONV-1298): the `.objects.create(
    # created_by_id=bot)` rows run `BaseModel.save` with no save kwarg and
    # no crum user in the worker, so Django NULLs both audit columns
    # (`db/models/base.py:31-33`). The explicit IssueSequence create is one
    # of the two per-issue rows; the `Issue.save` auto-row is NULL-audit on
    # both backends already, so asserting every project sequence row NULL
    # pins the explicit row too.
    link_rows = (
        _seed_project_rows(db_conn, "issue_labels", project["id"])
        + _seed_project_rows(db_conn, "cycle_issues", project["id"])
        + _seed_project_rows(db_conn, "module_issues", project["id"])
        + _seed_project_rows(db_conn, "issue_sequences", project["id"])
    )
    assert len(link_rows) == (
        expected_labels + expected_cycles + expected_modules + 2 * len(seed_issues)
    )
    assert all(r["created_by_id"] is None for r in link_rows)
    assert all(r["updated_by_id"] is None for r in link_rows)

    # Views: empty seed filters store an empty query; first view keeps its
    # seed sort.
    views = _seed_project_rows(db_conn, "issue_views", project["id"])
    assert len(views) == len(seed_views)
    assert views[0]["query"] == {}
    assert views[0]["sort_order"] == seed_views[0]["sort_order"]
    assert views[0]["owned_by_id"] == bot["id"]

    # Pages: both seed rows are PROJECT type, so both link.
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            'SELECT * FROM "pages" WHERE workspace_id = %s::uuid AND deleted_at IS NULL',
            (str(workspace["id"]),),
        )
        ws_pages = cur.fetchall()
    assert len(ws_pages) == len(seed_pages)
    assert {p["access"] for p in ws_pages} == {r["access"] for r in seed_pages}
    assert (
        _seed_count(
            db_conn,
            "project_pages",
            "WHERE project_id = %s::uuid",
            (str(project["id"]),),
        )
        == expected_project_pages
    )

    # Default pod + orchestration (PIDASHCONV-1297): the project-save
    # post_save creates {IDENT}_pod_1, each issue-save resolves
    # assigned_pod through it, and the post-save transition fires
    # (ticker + dispatch or preflight bounce).
    pods = _seed_project_rows(db_conn, "pod", project["id"])
    assert len(pods) == 1
    pod = pods[0]
    assert pod["name"] == f"{project['identifier']}_pod_1"
    assert pod["is_default"] is True
    assert pod["description"] == "Auto-created default pod. Add tier pods anytime."
    assert pod["created_by_id"] is None
    assert all(i["assigned_pod_id"] == pod["id"] for i in issues)

    # Ticking seed issues: agent_phases.PHASES pins the literal
    # (name, group) pairs, so the test names them (no Django import).
    # Sequence ids are 1-based in seed-file order (pinned above).
    ticking = {("In Progress", "started"), ("In Review", "review"), ("In Test", "test")}
    ticking_seed_states = {
        s["id"] for s in seed_states if (s["name"], s.get("group")) in ticking
    }
    by_sequence = {i["sequence_id"]: i for i in issues}
    ticking_n = [
        n
        for n, row in enumerate(seed_issues, start=1)
        if row["state_id"] in ticking_seed_states
    ]
    assert ticking_n, "seed data must carry at least one ticking-state issue"
    for n in ticking_n:
        issue = by_sequence[n]
        with db_conn.cursor(row_factory=dict_row) as cur:
            cur.execute(
                'SELECT * FROM "issue_agent_ticker" WHERE issue_id = %s::uuid',
                (str(issue["id"]),),
            )
            ticker = cur.fetchone()
        assert ticker is not None, f"issue {issue['id']} must own a ticker row"
        runs = _seed_count(
            db_conn, "agent_run", "WHERE work_item_id = %s::uuid", (str(issue["id"]),)
        )
        if runs:
            continue  # dispatched: the backend had an eligible runner
        # Bounced: Backlog state + the no-eligible-runner system comment.
        with db_conn.cursor(row_factory=dict_row) as cur:
            cur.execute(
                'SELECT "group" FROM "states" WHERE id = %s::uuid',
                (str(issue["state_id"]),),
            )
            group = cur.fetchone()["group"]
        assert group == "backlog", f"issue {issue['id']} must bounce to Backlog"
        with db_conn.cursor(row_factory=dict_row) as cur:
            cur.execute(
                'SELECT speaker_type, comment_html FROM "issue_comments" WHERE issue_id = %s::uuid',
                (str(issue["id"]),),
            )
            comments = cur.fetchall()
        assert any(
            c["speaker_type"] == "agent"
            and "Agent run skipped" in (c["comment_html"] or "")
            for c in comments
        ), f"issue {issue['id']} must carry the bounce comment"



def test_rust_dummy_data_before_after(db_conn):
    """PIDASHCONV-816: dummy-data task inserts full NOT NULL columns.

    The Rust executor historically inserted minimal column lists
    (project name/slug/identifier/workspace/audit only), crashing on
    ``projects.description`` NOT NULL. The fix ports every field the
    Python ``bulk_create``/``create`` calls persist (explicit values
    plus Django field defaults); this replay proves the worker path
    completes and every touched table carries the Python column
    shapes. Faker-dependent distinct counts (labels, modules) assert
    ranges: Python's seeded Faker yields 40 distinct label names, the
    Rust ``fake_*`` approximations are behaviourally equivalent but
    not sequence-identical.
    """
    tag = uuid.uuid4().hex[:8]
    owner = seed_helpers.user(db_conn, f"rust-replay-dummy-owner-{tag}")
    workspace = seed_helpers.workspace(
        db_conn,
        f"rustreplaydummy{tag}",
        owner["id"],
        name=f"Dummy Rust Replay {tag}",
    )
    seed_helpers.workspace_member(db_conn, workspace["id"], owner["id"], role=5)

    job_id = rust_queue.publish(
        f"{M}.dummy_data_task.create_dummy_data",
        args=[workspace["slug"], owner["email"], [], 6, 1, 5, 2, 2],
    )

    # Row counts fire mid-job (issues land before intakes and links),
    # so wait for the ack itself: success deletes the row, a spent
    # retry budget parks it as `failed` with the error text.
    def _job_settled():
        with db_conn.cursor() as cur:
            cur.execute(
                "SELECT status, last_error FROM rust_job_queue WHERE celery_id = %s",
                (job_id,),
            )
            row = cur.fetchone()
        if row is None:
            return True
        if row[0] == "failed":
            raise AssertionError(f"rust dummy job failed: {row[1]}")
        return None

    wait_for(_job_settled, what="rust dummy job acked")

    assert (
        _seed_count(
            db_conn,
            "projects",
            "WHERE workspace_id = %s::uuid AND deleted_at IS NULL",
            (str(workspace["id"]),),
        )
        == 1
    )
    project = _seed_one(
        db_conn,
        "projects",
        "WHERE workspace_id = %s::uuid AND deleted_at IS NULL",
        (str(workspace["id"]),),
    )

    # Project carries the Django field defaults (the task passes only
    # workspace/name/identifier/created_by/intake_view; `save()` nulls
    # the audit user, inherits the workspace timezone, and defaults
    # the first project).
    assert project["description"] == ""
    assert project["network"] == 2
    assert project["identifier"] == project["identifier"].strip().upper()
    assert project["identifier"] != ""
    assert project["project_lead_id"] is None
    assert project["default_state_id"] is None
    assert project["cover_image"] is None
    assert project["logo_props"] == {}
    assert project["estimate_id"] is None
    assert project["is_default"] is True
    assert project["intake_view"] is True
    assert project["default_agent_executor"] == "local_runner"
    assert project["timezone"] == workspace["timezone"]
    assert project["created_by_id"] is None
    assert project["updated_by_id"] is None

    pid = str(project["id"])
    # Five states with the Backlog default; labels attempt 50 rows
    # with conflicts ignored.
    assert _seed_count(db_conn, "states", "WHERE project_id = %s::uuid", (pid,)) == 5
    assert (
        _seed_count(
            db_conn, "states", "WHERE project_id = %s::uuid AND \"default\"", (pid,)
        )
        == 1
    )
    label_count = _seed_count(
        db_conn, "labels", "WHERE project_id = %s::uuid", (pid,)
    )
    assert 1 <= label_count <= 50
    # Creator member plus the save-hook property row.
    assert (
        _seed_count(
            db_conn, "project_members", "WHERE project_id = %s::uuid", (pid,)
        )
        == 1
    )
    assert (
        _seed_count(
            db_conn, "project_user_properties", "WHERE project_id = %s::uuid", (pid,)
        )
        == 1
    )
    # Cycles keep the `<=` off-by-one (1 -> 2 rows); modules keep
    # `range(module_count)` up to name conflicts.
    assert _seed_count(db_conn, "cycles", "WHERE project_id = %s::uuid", (pid,)) == 2
    module_count = _seed_count(
        db_conn, "modules", "WHERE project_id = %s::uuid", (pid,)
    )
    assert 1 <= module_count <= 5
    assert _seed_count(db_conn, "pages", "WHERE workspace_id = %s::uuid", (str(workspace["id"]),)) == 2
    assert (
        _seed_count(db_conn, "project_pages", "WHERE project_id = %s::uuid", (pid,))
        == 2
    )
    # create_issues runs twice (6 + 2 intakes): sequences and
    # activities follow every issue.
    assert (
        _seed_count(db_conn, "issue_sequences", "WHERE project_id = %s::uuid", (pid,))
        == 8
    )
    assert (
        _seed_count(db_conn, "issue_activities", "WHERE project_id = %s::uuid", (pid,))
        == 8
    )
    assert (
        _seed_count(db_conn, "intakes", "WHERE project_id = %s::uuid", (pid,)) == 1
    )
    assert (
        _seed_count(db_conn, "intake_issues", "WHERE project_id = %s::uuid", (pid,))
        == 2
    )

    # Intake issues are ordinary issues linked through intake_issues.
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            'SELECT MAX(sequence_id) AS top FROM "issues" WHERE project_id = %s::uuid',
            (pid,),
        )
        assert cur.fetchone()["top"] == 8


# ---------------------------------------------------------------------------
# PIDASHCONV-1292: S3-backed asset tasks replayed against the Rust worker.
# ---------------------------------------------------------------------------

_RUST_COPY_TASK = f"{M}.copy_s3_object.copy_s3_objects_of_description_and_assets"
_RUST_METADATA_TASK = f"{M}.storage_metadata_task.get_asset_object_metadata"


def _rust_page_html(conn, page_id) -> str:
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute('SELECT description_html FROM "pages" WHERE id = %s::uuid', (str(page_id),))
        return cur.fetchone()["description_html"]


def _rust_asset_row(conn, asset_id):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            'SELECT id, asset, is_uploaded, storage_metadata FROM "file_assets"'
            " WHERE id = %s::uuid AND deleted_at IS NULL",
            (str(asset_id),),
        )
        return cur.fetchone()


def _rust_image_srcs(html: str) -> list:
    return re.findall(r'<image-component\s+src="([^"]+)"', html or "")


def test_rust_copy_task_copies_s3_objects_and_rewrites_page(db_conn):
    """Rust replay of test_copy_task_copies_s3_objects_and_rewrites_page.

    Same seeds, same payload, same assertions through ``rust_queue``: every
    rewritten src resolves to a file_assets row whose S3 object exists.
    Red while the worker registers the copy handler with the
    ``UnavailableObjectStore`` stub (rows rewritten, keys never copied);
    green once the live store ships (PIDASHCONV-1292).
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"rcopy1292-{tag}")
    ws_id = str(chain["workspace"]["id"])
    proj_id = str(chain["project"]["id"])
    bucket = s3_helpers.contract_bucket()
    s3_helpers.s3_setup_bucket(bucket)
    body = b"1292-copy-probe-image"
    src_key = f"{ws_id}/rcopy1292-{tag}.png"
    s3_helpers.s3_put_key(bucket, src_key, body)
    assert s3_helpers.s3_key_exists(bucket, src_key)
    asset = insert_row(
        db_conn,
        "file_assets",
        {
            "asset": src_key,
            "attributes": {"name": f"rcopy1292-{tag}.png", "type": "png", "size": len(body)},
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
            "name": f"rcopy1292-{tag}",
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
    page_id = str(page["id"])
    source_id = str(asset["id"])
    rust_queue.publish(
        _RUST_COPY_TASK,
        kwargs={
            "entity_name": "PAGE",
            "entity_identifier": page_id,
            "project_id": proj_id,
            "slug": chain["workspace"]["slug"],
            "user_id": str(chain["owner"]["id"]),
        },
    )
    assert wait_for(
        lambda: _rust_page_html(db_conn, page_id) != seed_html,
        what="rust copy task rewrote the page description",
    )
    srcs = _rust_image_srcs(_rust_page_html(db_conn, page_id))
    assert srcs, "rewrite removed every image-component"
    assert source_id not in srcs, "srcs still point at the source asset"
    for src in srcs:
        row = _rust_asset_row(db_conn, src)
        assert row is not None, f"src {src} resolves to no file_assets row"
        assert row["is_uploaded"] is True, f"duplicate {src} never marked uploaded"
        assert s3_helpers.s3_key_exists(bucket, row["asset"]), (
            f"duplicate {src} points at S3 key {row['asset']} with no object behind it"
        )
    assert s3_helpers.s3_key_exists(bucket, src_key)


def test_rust_metadata_task_stores_head_dict(db_conn):
    """Rust replay of test_metadata_task_stores_head_dict.

    Same seeds, same payload, same assertions through ``rust_queue``:
    storage_metadata lands the Django-shaped head dict, never SQL NULL.
    Red while the worker registers the metadata handler with the
    ``UnavailableObjectStore`` stub (every head answers None);
    green once the live store ships (PIDASHCONV-1292).
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"rmeta1292-{tag}")
    ws_id = str(chain["workspace"]["id"])
    bucket = s3_helpers.contract_bucket()
    s3_helpers.s3_setup_bucket(bucket)
    body = b"1292-metadata-probe-image!"
    key = f"{ws_id}/rmeta1292-{tag}.png"
    s3_helpers.s3_put_key(bucket, key, body)
    assert s3_helpers.s3_key_exists(bucket, key)
    asset = insert_row(
        db_conn,
        "file_assets",
        {
            "asset": key,
            "attributes": {"name": f"rmeta1292-{tag}.png", "type": "png", "size": len(body)},
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
    rust_queue.publish(_RUST_METADATA_TASK, kwargs={"asset_id": asset_id})
    assert wait_for(
        lambda: (_rust_asset_row(db_conn, asset_id) or {}).get("storage_metadata") != {},
        what="rust metadata task saved storage_metadata",
    )
    meta = _rust_asset_row(db_conn, asset_id)["storage_metadata"]
    assert meta is not None, "storage_metadata saved as SQL NULL"
    assert set(meta) == {"ContentType", "ContentLength", "LastModified", "ETag", "Metadata"}
    assert meta["ContentLength"] == len(body)
    assert meta["ContentType"] == "application/octet-stream"
    assert meta["ETag"], "missing ETag"
    assert meta["LastModified"].endswith("+00:00"), meta["LastModified"]


def _assert_rust_soft_delete_acked(conn, celery_id: str, model_name: str) -> None:
    """Terminal-state poll: the row is gone (ack) or parked failed.

    A `Verdict::Fail` (e.g. the pre-fix `UnknownModel`) parks
    immediately, so this fails fast with the worker's error text instead
    of timing out on the missing stamp.
    """

    def terminal():
        if not rust_queue.row_present(celery_id):
            return "acked"
        failure = _queue_failure(conn, celery_id)
        return f"failed: {failure}" if failure else None

    outcome = wait_for(
        terminal, what=f"soft-delete of {model_name} settled by rust worker"
    )
    assert outcome == "acked", f"rust worker did not ack {model_name}: {outcome}"


def test_rust_soft_delete_draftissue_cascades(db_conn):
    """PIDASHCONV-1293: the Rust worker resolves ("db", "draftissue") and
    stamps the CASCADE children plus the draft itself, exactly like the
    Django oracle. Pre-fix this job parked as UnknownModel and every
    child row stayed live.
    """
    tag = uuid.uuid4().hex[:8]
    pk, extra = seed_draft_tree(db_conn, tag)
    celery_id = rust_queue.publish(
        f"{M}.deletion_task.soft_delete_related_objects",
        args=["db", "draftissue", str(pk)],
        kwargs={"using": None},
    )
    _assert_rust_soft_delete_acked(db_conn, celery_id, "draftissue")
    assert _soft_deleted(db_conn, "draft_issues", pk), "draft row not stamped"
    for table, child_id in extra["children"]:
        assert _soft_deleted(db_conn, table, child_id), (
            f"cascade parity broken: live {table} row under a stamped draft"
        )


@pytest.mark.parametrize(
    "model_name,table,seeder", ENTRY_CASES, ids=[case[0] for case in ENTRY_CASES]
)
def test_rust_soft_delete_entry_models_ack(
    db_conn, model_name, table, seeder
):
    """PIDASHCONV-1293: every model name the Rust API enqueues resolves —
    the job acks and stamps the row instead of parking as UnknownModel.
    The draft case re-pins the CASCADE children; the schedulerbinding
    case pins the `agent_run.scheduler_binding_id` SET_NULL.
    """
    tag = uuid.uuid4().hex[:8]
    pk, extra = seeder(db_conn, tag)
    celery_id = rust_queue.publish(
        f"{M}.deletion_task.soft_delete_related_objects",
        args=["db", model_name, str(pk)],
        kwargs={"using": None},
    )
    _assert_rust_soft_delete_acked(db_conn, celery_id, model_name)
    assert _soft_deleted(db_conn, table, pk), f"{table} row not stamped"
    for child_table, child_id in extra.get("children", []):
        assert _soft_deleted(db_conn, child_table, child_id), (
            f"cascade parity broken: live {child_table} row under stamped {model_name}"
        )
    if "agent_run_id" in extra:
        assert _column_is_null(
            db_conn, "agent_run", extra["agent_run_id"], "scheduler_binding_id"
        ), "SET_NULL parity broken: agent_run still points at the stamped binding"


def test_rust_sync_description_version_large_workspace(db_conn):
    """Rust replay of `test_sync_description_version_large_workspace`.

    Same seeds and assertions through `rust_queue`: 4,400 live issues in
    one workspace, sync at the default batch_size, exactly one
    `issue_description_versions` row per seeded issue (PIDASHCONV-1294).
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.bulk_issues(db_conn, f"1294rdesc-{tag}", 4400)
    wid = chain["workspace"]["id"]
    rust_queue.publish(
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
def test_rust_track_page_version_description_stripped(db_conn, html):
    """Rust track_page_version stores HTMLParser text (PIDASHCONV-1296).

    Replay of ``test_track_page_version_description_stripped``: entities
    decoded (``a & b``, not ``a &amp; b``), a bare ``<`` kept as text.
    """
    tag = uuid.uuid4().hex[:8]
    owner = seed_helpers.user(db_conn, f"rust-trackver-{tag}")
    workspace = seed_helpers.workspace(db_conn, f"rusttrackver{tag}", owner["id"])
    page = insert_row(
        db_conn,
        "pages",
        {
            "workspace_id": str(workspace["id"]),
            "owned_by_id": str(owner["id"]),
            "name": f"rust track version {tag}",
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
    rust_queue.publish(
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

    row = wait_for(_version_row, what="page_version stripped by rust worker")
    assert row["description_stripped"] == _ml_strip(html)


def test_rust_description_version_task_writes_null_audit(db_conn):
    """Rust issue_description_version_task inserts NULL audit cols (PIDASHCONV-1295).

    Replay of ``test_description_version_task_writes_null_audit``: the
    task-path insert NULLs ``created_by_id`` and ``updated_by_id`` (the
    crum rule, base.py:31-33) while ``owned_by_id`` keeps the acting user.
    The sync backfill keeps the issue's audit copies (``bulk_create``
    skips ``save()``), so only the task path is pinned here.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"1295rdesc-{tag}")
    owner = chain["owner"]
    issue = chain["issue"]
    updater = seed_helpers.user(db_conn, f"rust-1295desc-{tag}-updater")
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET created_by_id = %s::uuid,"
            " updated_by_id = %s::uuid,"
            " description_html = '<p>live v1</p>'"
            " WHERE id = %s::uuid",
            (str(owner["id"]), str(updater["id"]), str(issue["id"])),
        )
    rust_queue.publish(
        f"{M}.issue_description_version_task.issue_description_version_task",
        args=[
            _json.dumps({"description_html": "<p>stale</p>"}),
            str(issue["id"]),
            str(owner["id"]),
        ],
        kwargs={"is_creating": True},
    )

    def _version_row():
        with db_conn.cursor(row_factory=dict_row) as cur:
            cur.execute(
                "SELECT created_by_id, updated_by_id, owned_by_id"
                " FROM issue_description_versions"
                " WHERE issue_id = %s::uuid",
                (str(issue["id"]),),
            )
            return cur.fetchone()

    row = wait_for(_version_row, what="description version by rust worker")
    assert row["created_by_id"] is None
    assert row["updated_by_id"] is None
    assert str(row["owned_by_id"]) == str(owner["id"])


# The scheduler reclaims stale running rows at most once per minute, so a
# row seeded just after a sweep waits nearly a full minute. The bound
# leaves headroom for loaded CI without letting a never-reclaimed row
# (the pre-fix behavior) pass.
RECLAIM_BOUND_SECS = 150


def _seed_stale_running(db_conn, celery_id: str, task: str) -> None:
    """A dead worker's footprint: claimed 2h ago, never settled."""
    stale = datetime.now(timezone.utc) - timedelta(hours=2)
    with db_conn.cursor() as cur:
        cur.execute(
            f"INSERT INTO {rust_queue.QUEUE_TABLE} "
            "(celery_id, task, args, kwargs, queue, status, "
            "claimed_at, claimed_by) "
            "VALUES (%s, %s, '[]', '{}', 'celery', 'running', %s, %s)",
            (celery_id, task, stale, "contract-dead-worker"),
        )


def _queue_row(db_conn, celery_id: str):
    with db_conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            f"SELECT status, attempts, last_error FROM {rust_queue.QUEUE_TABLE} "
            "WHERE celery_id = %s",
            (celery_id,),
        )
        return cur.fetchone()


def test_1327_rust_reclaims_stale_running_rows(db_conn):
    """PIDASHCONV-1327 replay: stale ``running`` rows are reclaimed.

    A row whose worker died mid-job (panic, SIGKILL, crash, redeploy)
    must not sit ``running`` forever: the scheduler reclaims it past the
    lease, and the worker then processes it normally. The unknown-name
    row parks ``failed`` with its cause; the no-op cleanup row runs and
    acks. Before the fix both rows stay ``running`` with NULL
    ``last_error`` and this test times out at its bound.
    """
    tag = uuid.uuid4().hex[:8]
    unknown_id = f"1327-unknown-{tag}"
    noop_id = f"1327-noop-{tag}"
    _seed_stale_running(db_conn, unknown_id, f"test.unknown.1327-{tag}")
    _seed_stale_running(db_conn, noop_id, f"{M}.cleanup_task.delete_api_logs")

    def _unknown_parked():
        row = _queue_row(db_conn, unknown_id)
        return row if row and row["status"] == "failed" else None

    parked = wait_for(
        _unknown_parked,
        timeout=RECLAIM_BOUND_SECS,
        what="stale unknown-task row reclaimed and parked",
    )
    assert parked["attempts"] >= 1
    assert parked["last_error"] and "unknown task" in parked["last_error"]

    def _noop_acked():
        return True if _queue_row(db_conn, noop_id) is None else None

    wait_for(
        _noop_acked,
        timeout=RECLAIM_BOUND_SECS,
        what="stale no-op row reclaimed and acked",
    )
