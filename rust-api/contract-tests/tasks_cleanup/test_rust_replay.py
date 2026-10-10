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
    export CELERY_BROKER_URL=amqp://...   # same broker the Rust worker uses
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
  unuploaded-asset soft delete, hard delete, ETA delay, redelivery,
  workspace_seed before/after. No skipped or weakened assertions.
- The 4 export tasks stay Python-owned (``is_export_task`` /
  ``EXPORT_TASK_NAMES``): the Rust worker forwards them to AMQP. Their
  replay form asserts observable forwarding — the ``rust_job_queue``
  row is acked AND the broker ``celery`` queue grows while the Django
  worker is stopped — instead of local DB effects.
"""

import json as _json
import uuid
from datetime import datetime, timedelta, timezone

import pytest
from psycopg.rows import dict_row

from _harness import broker_probe, rust_queue, taskspec as _taskspec
from _harness import seed as seed_helpers
from _harness.db import insert_row, snapshot, wait_for_condition as wait_for

M = "pi_dash.bgtasks"

EXPORT_TASKS = [
    f"{M}.export_task.issue_export_task",
    f"{M}.exporter_expired_task.delete_old_s3_link",
    f"{M}.analytic_plot_export.analytic_export_task",
    f"{M}.analytic_plot_export.export_analytics_to_csv_email",
]


def test_rust_worker_liveness(db_conn, broker_url):
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


def test_rust_delete_api_logs(db_conn, broker_url):
    row = insert_row(
        db_conn,
        "api_activity_logs",
        {
            "token_identifier": "rust-replay-probe",
            "path": "/contract/old",
            "method": "GET",
            "response_code": 200,
            "created_at": _old(),
        },
    )
    rust_queue.publish(f"{M}.cleanup_task.delete_api_logs")
    wait_for(
        lambda: _missing(db_conn, "api_activity_logs", row["id"]),
        what="aged api_activity_logs row deleted by rust worker",
    )


def test_rust_delete_email_notification_logs(db_conn, broker_url):
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


def test_rust_delete_webhook_logs(db_conn, broker_url):
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


def test_rust_delete_page_versions_keeps_newest_20(db_conn, broker_url):
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


def test_rust_delete_unuploaded_file_asset(db_conn, broker_url):
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


def test_rust_hard_delete_removes_tombstoned_rows(db_conn, broker_url):
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


def test_rust_eta_delayed_execution(db_conn, broker_url):
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


def test_rust_cleanup_redelivery_deletes_nothing_twice(db_conn, broker_url):
    """Redelivery: a second run over an empty eligible set changes nothing."""
    before = snapshot(db_conn, ["api_activity_logs"])
    rust_queue.publish(f"{M}.cleanup_task.delete_api_logs")
    rust_queue.wait_for_queue_drain(what="first rust cleanup consumed")
    rust_queue.publish(f"{M}.cleanup_task.delete_api_logs")
    rust_queue.wait_for_queue_drain(what="redelivered rust cleanup consumed")
    from _harness.db import diff as _diff

    after = snapshot(db_conn, ["api_activity_logs"])
    assert _diff(before, after) == {}


def test_rust_storage_aware_tasks_consumed_without_crash(db_conn, broker_url):
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


def test_rust_exports_forward_to_python(db_conn, broker_url):
    """Export parity: the 4 Python-owned tasks forward over AMQP, observably.

    The Rust worker has no local handler for these names (``is_export_task``)
    while S3/SMTP and the exporter serializers still live on the Python
    plane. Each job's ``rust_job_queue`` row must therefore be acked via the
    forward path, AND its Celery v2 message must arrive on the broker
    ``celery`` queue — which grows because the Django worker is stopped.
    """
    probe_args = {
        EXPORT_TASKS[0]: (["csv", "ws-1", [], "tok-9", False, "slug"], {}),
        EXPORT_TASKS[1]: ([], {}),
        EXPORT_TASKS[2]: (["probe@example.com", {"metric": "count"}, "slug"], {}),
        EXPORT_TASKS[3]: (([{}], ["H"], ["k"], "probe@example.com", "slug"), {}),
    }
    baseline = broker_probe.queue_depth()
    celery_ids = []
    for task_name in EXPORT_TASKS:
        args, kwargs = probe_args[task_name]
        celery_ids.append(rust_queue.publish(task_name, args=list(args), kwargs=kwargs))
    for celery_id, task_name in zip(celery_ids, EXPORT_TASKS):
        rust_queue.wait_for_settled(celery_id, what=f"{task_name} forwarded")

    def _forwarded_depth():
        depth = broker_probe.queue_depth()
        return depth if depth >= baseline + len(EXPORT_TASKS) else None

    arrived = wait_for(
        _forwarded_depth,
        what="export messages arrived on broker for python plane",
    )
    assert arrived >= baseline + len(EXPORT_TASKS)


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


def test_rust_workspace_seed_before_after(db_conn, broker_url):
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

    rust_queue.publish(
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



def test_rust_dummy_data_before_after(db_conn, broker_url):
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
