"""Response-shape coverage for the D-27 cycle routes.

Every URL pattern in app/urls/cycle.py plus the workspace cycles endpoint in
app/urls/workspace.py gets a shape assertion against the live backend.
"""

import json
import os
import time
import uuid
from datetime import datetime, timedelta, timezone

import pytest

from _harness.checks import require_keys
from .conftest import create_cycle_for, create_issue_for, cycle_urls

pytestmark = pytest.mark.contract

# GET cycles/ rows and POST cycles/ responses carry the annotated counts but
# no cancelled_issues on create (the create path selects a narrower column
# list than list). Quirk of the Django contract; the Rust port must
# reproduce it byte for byte.
CYCLE_LIST_KEYS = {
    "id", "workspace_id", "project_id", "name", "description", "start_date",
    "end_date", "owned_by_id", "view_props", "sort_order", "external_source",
    "external_id", "progress_snapshot", "logo_props", "is_favorite",
    "total_issues", "cancelled_issues", "completed_issues", "assignee_ids",
    "status", "version", "created_by",
}

CYCLE_CREATE_KEYS = CYCLE_LIST_KEYS - {"cancelled_issues"}

# PUT cycles/<pk>/ falls through to the ModelViewSet default update, which
# serializes without the get_queryset annotations, so assignee_ids,
# created_by and version are absent. Same-contract quirk as above.
CYCLE_PUT_KEYS = {
    "id", "workspace_id", "project_id", "name", "description", "start_date",
    "end_date", "owned_by_id", "view_props", "sort_order", "external_source",
    "external_id", "progress_snapshot", "logo_props", "is_favorite",
    "total_issues", "cancelled_issues", "completed_issues", "status",
}

CYCLE_DETAIL_KEYS = CYCLE_CREATE_KEYS | {"sub_issues"}

ARCHIVED_DETAIL_KEYS = {
    "id", "workspace_id", "project_id", "name", "description", "start_date",
    "end_date", "owned_by_id", "view_props", "sort_order", "external_source",
    "external_id", "progress_snapshot", "logo_props", "is_favorite",
    "total_issues", "cancelled_issues", "completed_issues", "started_issues",
    "unstarted_issues", "backlog_issues", "assignee_ids", "status",
    "archived_at", "sub_issues", "distribution", "estimate_distribution",
    "completed_estimate_points", "total_estimate_points", "created_by",
}

# The workspace endpoint serializes with CycleSerializer but annotates only
# the issue counts, so is_favorite/status (annotations in the project
# viewset) are absent here. Same-contract quirk as above.
WORKSPACE_CYCLE_KEYS = {
    "id", "workspace_id", "project_id", "name", "description", "start_date",
    "end_date", "owned_by_id", "view_props", "sort_order", "external_source",
    "external_id", "progress_snapshot", "logo_props", "total_issues",
    "cancelled_issues", "completed_issues", "started_issues",
    "unstarted_issues", "backlog_issues",
}

PROGRESS_KEYS = {
    "backlog_estimate_points", "unstarted_estimate_points",
    "started_estimate_points", "cancelled_estimate_points",
    "completed_estimate_points", "total_estimate_points", "backlog_issues",
    "total_issues", "completed_issues", "cancelled_issues", "started_issues",
    "unstarted_issues",
}

ANALYTICS_KEYS = {"assignees", "labels", "completion_chart"}

USER_PROPERTIES_KEYS = {
    "id", "filters", "display_filters", "display_properties", "rich_filters",
    "cycle", "user", "project", "workspace", "created_by", "updated_by",
    "created_at", "updated_at", "deleted_at",
}

CYCLE_ISSUE_ROW_KEYS = {
    "id", "name", "priority", "sequence_id", "project_id", "state_id",
    "cycle_id", "assignee_ids", "label_ids", "module_ids", "sub_issues_count",
    "link_count", "attachment_count", "created_at", "updated_at",
}

GENERIC_500 = {"error": "Something went wrong please try again later"}


def _past_range(days_ago_start=30, days_ago_end=1):
    now = datetime.now(timezone.utc).date()
    return (
        (now - timedelta(days=days_ago_start)).isoformat(),
        (now - timedelta(days=days_ago_end)).isoformat(),
    )


def test_cycle_list_shape(admin):
    urls = cycle_urls(admin)
    r = admin["client"].get(urls["cycles"])
    assert r.status_code == 200, r.text[:500]
    rows = r.json()
    assert isinstance(rows, list) and rows, "expected at least the seeded cycle"
    require_keys(rows[0], CYCLE_LIST_KEYS, "GET cycles/ row")
    assert {row["id"] for row in rows} >= {admin["cycle"]["id"]}
    assert rows[0]["status"] == "DRAFT"


def test_cycle_create_shape(admin):
    urls = cycle_urls(admin)
    r = admin["client"].post(urls["cycles"], json={"name": "Second cycle"})
    assert r.status_code == 201, r.text[:500]
    body = r.json()
    require_keys(body, CYCLE_CREATE_KEYS, "POST cycles/")
    assert body["name"] == "Second cycle"
    assert body["status"] == "DRAFT"


def test_cycle_create_date_validation(admin):
    urls = cycle_urls(admin)
    r = admin["client"].post(
        urls["cycles"], json={"name": "Half", "start_date": "2026-01-01"}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "error": "Both start date and end date are either required or are to be null"
    }
    r = admin["client"].post(
        urls["cycles"],
        json={"name": "Bad", "start_date": "2026-06-01", "end_date": "2026-01-01"},
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"non_field_errors": ["Start date cannot exceed end date"]}


def test_cycle_retrieve_shape(admin):
    urls = cycle_urls(admin)
    r = admin["client"].get(urls["detail"](admin["cycle"]["id"]))
    assert r.status_code == 200, r.text[:500]
    require_keys(r.json(), CYCLE_DETAIL_KEYS, "GET cycles/<pk>/")


def test_cycle_retrieve_404(admin):
    urls = cycle_urls(admin)
    r = admin["client"].get(urls["detail"](uuid.uuid4()))
    assert r.status_code == 404, r.text[:500]
    assert r.json() == {"error": "Cycle not found"}


def test_cycle_patch_shape(admin):
    urls = cycle_urls(admin)
    r = admin["client"].patch(
        urls["detail"](admin["cycle"]["id"]), json={"name": "Renamed cycle"}
    )
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    require_keys(body, CYCLE_CREATE_KEYS, "PATCH cycles/<pk>/")
    assert body["name"] == "Renamed cycle"


def test_cycle_put_is_noop_upstream(admin):
    # PORTED BUG: PUT maps to the ModelViewSet default update, which uses
    # CycleSerializer whose Meta marks every field read_only — so validated
    # data is empty, save() changes nothing, and the endpoint answers 200
    # with the old values. Use PATCH for real updates. The contract pins
    # the no-op byte for byte; the Rust port must reproduce it, and the fix
    # belongs to the D-27 port issue, not here.
    urls = cycle_urls(admin)
    pk = admin["cycle"]["id"]
    before = admin["client"].get(urls["detail"](pk)).json()["name"]
    r = admin["client"].put(urls["detail"](pk), json={"name": "Put cycle"})
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    require_keys(body, CYCLE_PUT_KEYS, "PUT cycles/<pk>/")
    assert body["name"] == before
    assert admin["client"].get(urls["detail"](pk)).json()["name"] == before


def test_cycle_delete_shape(admin):
    row = create_cycle_for(admin, name="Disposable")
    urls = cycle_urls(admin)
    pk = row["id"]
    r = admin["client"].delete(urls["detail"](pk))
    assert r.status_code == 204, r.text[:500]
    assert admin["client"].get(urls["detail"](pk)).status_code == 404


def test_workspace_cycles_shape(admin):
    urls = cycle_urls(admin)
    r = admin["client"].get(urls["workspace_cycles"])
    assert r.status_code == 200, r.text[:500]
    rows = r.json()
    assert isinstance(rows, list) and rows
    require_keys(rows[0], WORKSPACE_CYCLE_KEYS, "GET workspaces/<slug>/cycles/")
    assert {row["id"] for row in rows} >= {admin["cycle"]["id"]}


def test_date_check_open_conflict_and_missing(admin):
    urls = cycle_urls(admin)
    start, end = _past_range(90, 80)
    r = admin["client"].post(
        urls["date_check"], json={"start_date": start, "end_date": end}
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json() == {"status": True}

    # Plant a cycle on those dates, then the same range conflicts.
    created = admin["client"].post(
        urls["cycles"],
        json={"name": "Blocker", "start_date": start, "end_date": end},
    )
    assert created.status_code == 201, created.text[:500]
    r = admin["client"].post(
        urls["date_check"], json={"start_date": start, "end_date": end}
    )
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    assert body["status"] is False
    assert body["error"].startswith("You have a cycle already on the given dates")

    r = admin["client"].post(urls["date_check"], json={})
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"error": "Start date and end date both are required"}


def test_date_check_rejects_dates_python_strptime_rejects(admin):
    """CPython's `%Y-%m-%d` is anchored and strict: not four digits for the
    year, a sign, or a leading space before the month is a ValueError, which
    Django's `convert_to_utc` lets escape as the generic 500 (PB-0112). The
    Rust port must answer the same 500, never a 200 overlap verdict."""
    urls = cycle_urls(admin)
    for start in (" 2026-03-01", "26-03-01", "+2026-03-01", "2026- 3-01"):
        r = admin["client"].post(
            urls["date_check"],
            json={"start_date": start, "end_date": "2026-03-31"},
        )
        assert r.status_code == 500, (start, r.status_code, r.text[:200])
        assert r.json() == {
            "error": "Something went wrong please try again later"
        }, start


def test_date_check_accepts_dates_python_strptime_accepts(admin):
    """The forms both parsers accept: a 1-digit month/day and a
    space-padded single-digit day. Both answer 200 with the clear verdict
    (the isolated tenant has no cycle on those dates)."""
    urls = cycle_urls(admin)
    for start in ("2026-3-1", "2026-03- 1"):
        r = admin["client"].post(
            urls["date_check"],
            json={"start_date": start, "end_date": "2026-03-31"},
        )
        assert r.status_code == 200, (start, r.status_code, r.text[:200])
        assert r.json() == {"status": True}, start


def test_progress_shape(admin):
    urls = cycle_urls(admin)
    r = admin["client"].get(urls["progress"](admin["cycle"]["id"]))
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    require_keys(body, PROGRESS_KEYS, "GET cycles/<pk>/progress/")
    assert body["total_issues"] == 0


def test_progress_404(admin):
    urls = cycle_urls(admin)
    r = admin["client"].get(urls["progress"](uuid.uuid4()))
    assert r.status_code == 404, r.text[:500]
    assert r.json() == {"error": "Cycle not found"}


def test_analytics_requires_dates(admin):
    urls = cycle_urls(admin)
    r = admin["client"].get(urls["analytics"](admin["cycle"]["id"]))
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"error": "Cycle has no start or end date"}


def test_analytics_shape(admin):
    urls = cycle_urls(admin)
    start, end = _past_range(60, 50)
    r = admin["client"].post(
        urls["cycles"],
        json={"name": "Analytic", "start_date": start, "end_date": end},
    )
    assert r.status_code == 201, r.text[:500]
    pk = r.json()["id"]
    r = admin["client"].get(urls["analytics"](pk))
    assert r.status_code == 200, r.text[:500]
    require_keys(r.json(), ANALYTICS_KEYS, "GET cycles/<pk>/analytics/")


def test_progress_soft_deleted_cycle_404(admin):
    """`Cycle.objects` is the soft-deletion manager (`mixins.py:61-67`), so a
    soft-deleted cycle reads as missing and progress answers 404, like an
    unknown id (`base.py:661-663`)."""
    urls = cycle_urls(admin)
    pk = create_cycle_for(admin, name="Soft-deleted progress")["id"]
    assert admin["client"].delete(urls["detail"](pk)).status_code == 204
    r = admin["client"].get(urls["progress"](pk))
    assert r.status_code == 404, r.text[:500]
    assert r.json() == {"error": "Cycle not found"}


def test_analytics_soft_deleted_cycle_500(admin):
    """A soft-deleted cycle takes the same path as an unknown id: the
    `.first()` lookup is None and the `cycle.start_date` dereference 500s
    (`base.py:789-807`), for every `?type=` including `points`."""
    urls = cycle_urls(admin)
    start, end = _past_range(60, 50)
    r = admin["client"].post(
        urls["cycles"],
        json={
            "name": "Soft-deleted analytics",
            "start_date": start,
            "end_date": end,
        },
    )
    assert r.status_code == 201, r.text[:500]
    pk = r.json()["id"]
    assert admin["client"].delete(urls["detail"](pk)).status_code == 204
    for params in ({}, {"type": "points"}):
        r = admin["client"].get(urls["analytics"](pk), params=params)
        assert r.status_code == 500, r.text[:500]
        assert r.json() == {
            "error": "Something went wrong please try again later"
        }


def test_user_properties_shapes(admin):
    urls = cycle_urls(admin)
    pk = admin["cycle"]["id"]
    r = admin["client"].get(urls["user_properties"](pk))
    assert r.status_code == 200, r.text[:500]
    require_keys(r.json(), USER_PROPERTIES_KEYS, "GET user-properties/")
    # PATCH answers 201 (not 200) for an update. Quirk of the Django
    # contract; the Rust port must reproduce it byte for byte.
    r = admin["client"].patch(
        urls["user_properties"](pk), json={"filters": {"priority": "high"}}
    )
    assert r.status_code == 201, r.text[:500]
    body = r.json()
    require_keys(body, USER_PROPERTIES_KEYS, "PATCH user-properties/")
    assert body["filters"] == {"priority": "high"}


def test_favorite_create_destroy(admin):
    urls = cycle_urls(admin)
    pk = admin["cycle"]["id"]
    r = admin["client"].post(urls["favorites"], json={"cycle": pk})
    assert r.status_code == 204, r.text[:500]
    r = admin["client"].delete(urls["favorite_detail"](pk))
    assert r.status_code == 204, r.text[:500]
    r = admin["client"].delete(urls["favorite_detail"](pk))
    assert r.status_code == 404, r.text[:500]
    assert r.json() == {"error": "The required object does not exist."}


def test_favorite_list_broken_upstream(admin):
    # PORTED BUG (app/views/cycle/base.py): CycleFavoriteViewSet defines no
    # list method, so GET falls through to the ModelViewSet default, which
    # has no serializer_class and dies with a 500 for every caller. The
    # contract pins the 500 + body byte for byte; the Rust port must
    # reproduce it, and the fix belongs to the D-27 port issue, not here.
    urls = cycle_urls(admin)
    r = admin["client"].get(urls["favorites"])
    assert r.status_code == 500, r.text[:500]
    assert r.json() == GENERIC_500


def test_cycle_issue_add_list_remove(admin):
    urls = cycle_urls(admin)
    pk = admin["cycle"]["id"]
    issue = create_issue_for(admin)
    r = admin["client"].post(
        urls["cycle_issues"](pk), json={"issues": [issue["id"]]}
    )
    assert r.status_code == 201, r.text[:500]
    assert r.json() == {"message": "success"}

    r = admin["client"].get(urls["cycle_issues"](pk))
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    assert body["total_count"] >= 1
    require_keys(body["results"][0], CYCLE_ISSUE_ROW_KEYS, "GET cycle-issues/ row")
    assert {row["id"] for row in body["results"]} >= {issue["id"]}

    r = admin["client"].delete(urls["cycle_issue_detail"](pk, issue["id"]))
    assert r.status_code == 204, r.text[:500]


def _bridge_row(tenant, issue_id):
    """Read the live bridge row straight from Postgres (both backends share
    the canonical schema)."""
    with tenant["seed"].conn.cursor() as cur:
        cur.execute(
            "select cycle_id, created_at, updated_at from cycle_issues "
            "where issue_id = %s and deleted_at is null",
            (issue_id,),
        )
        return cur.fetchone()


def test_cycle_issue_move_keeps_updated_at(admin):
    """Moving an existing bridge to another cycle leaves updated_at alone.

    Django's CycleIssueViewSet.create sets ``cycle_issue.cycle_id`` and calls
    ``bulk_update(updated_records, ["cycle_id"], batch_size=100)``
    (app/views/cycle/issue.py:262-279); bulk_update never runs pre_save, so
    the ``auto_now`` updated_at is not advanced. The Rust move must touch
    only cycle_id, exactly like the transfer move in the same slice.
    """
    urls = cycle_urls(admin)
    issue = create_issue_for(admin, name="Move between cycles")
    cycle_a = create_cycle_for(admin, name="Move source")
    cycle_b = create_cycle_for(admin, name="Move target")

    r = admin["client"].post(
        urls["cycle_issues"](cycle_a["id"]), json={"issues": [issue["id"]]}
    )
    assert r.status_code == 201, r.text[:500]

    before = _bridge_row(admin, issue["id"])
    assert before is not None, "bridge not created"
    assert str(before[0]) == cycle_a["id"], "bridge not in cycle A"

    # The move must be observable as a distinct timestamp if updated_at were
    # (wrongly) advanced, so leave a clear gap.
    time.sleep(1.1)

    r = admin["client"].post(
        urls["cycle_issues"](cycle_b["id"]), json={"issues": [issue["id"]]}
    )
    assert r.status_code == 201, r.text[:500]

    after = _bridge_row(admin, issue["id"])
    assert after is not None, "bridge vanished"
    assert str(after[0]) == cycle_b["id"], "bridge did not move to cycle B"
    assert after[2] == before[2], (
        "bulk_update(['cycle_id']) must not advance the bridge updated_at"
    )


def test_cycle_issue_create_requires_issues(admin):
    urls = cycle_urls(admin)
    r = admin["client"].post(urls["cycle_issues"](admin["cycle"]["id"]), json={})
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"error": "Issues are required"}


def test_cycle_issue_detail_broken_upstream(admin):
    # PORTED BUG (app/views/cycle/issue.py): CycleIssueViewSet implements
    # only list/create/destroy, so the retrieve/update/partial_update actions
    # the URLconf maps fall through to the ModelViewSet defaults, which die
    # with a 500 for every caller. Pinned byte for byte like the favorite
    # list bug above.
    urls = cycle_urls(admin)
    pk = admin["cycle"]["id"]
    issue = create_issue_for(admin, name="Detail bug issue")
    admin["client"].post(urls["cycle_issues"](pk), json={"issues": [issue["id"]]})
    url = urls["cycle_issue_detail"](pk, issue["id"])
    for method in ("get", "patch", "put"):
        if method == "get":
            r = admin["client"].get(url)
        elif method == "patch":
            r = admin["client"].patch(url, json={"name": "x"})
        else:
            r = admin["client"].put(url, json={"name": "x"})
        assert r.status_code == 500, f"{method}: {r.text[:500]}"
        assert r.json() == GENERIC_500, f"{method}: {r.text[:500]}"


def test_transfer_requires_new_cycle(admin):
    urls = cycle_urls(admin)
    r = admin["client"].post(
        urls["transfer"](admin["cycle"]["id"]), json={}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"error": "New Cycle Id is required"}


def test_transfer_success_moves_issue(admin):
    urls = cycle_urls(admin)
    issue = create_issue_for(admin, name="Transfer me")
    source = admin["client"].post(urls["cycles"], json={"name": "Source"})
    assert source.status_code == 201, source.text[:500]
    dest = admin["client"].post(urls["cycles"], json={"name": "Dest"})
    assert dest.status_code == 201, dest.text[:500]
    src_id, dst_id = source.json()["id"], dest.json()["id"]
    r = admin["client"].post(
        urls["cycle_issues"](src_id), json={"issues": [issue["id"]]}
    )
    assert r.status_code == 201, r.text[:500]
    r = admin["client"].post(
        urls["transfer"](src_id), json={"new_cycle_id": dst_id}
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json() == {"message": "Success"}
    r = admin["client"].get(urls["cycle_issues"](dst_id))
    assert r.status_code == 200, r.text[:500]
    assert {row["id"] for row in r.json()["results"]} >= {issue["id"]}


def test_transfer_bogus_target_500(admin):
    # PORTED BUG: an unknown new_cycle_id makes the transfer util raise
    # (DoesNotExist) instead of answering 400/404. Pinned byte for byte.
    urls = cycle_urls(admin)
    r = admin["client"].post(
        urls["transfer"](admin["cycle"]["id"]),
        json={"new_cycle_id": str(uuid.uuid4())},
    )
    assert r.status_code == 500, r.text[:500]
    assert r.json() == GENERIC_500


def test_archive_flow(admin):
    urls = cycle_urls(admin)
    start, end = _past_range(30, 1)
    r = admin["client"].post(
        urls["cycles"],
        json={"name": "Finish me", "start_date": start, "end_date": end},
    )
    assert r.status_code == 201, r.text[:500]
    pk = r.json()["id"]

    r = admin["client"].post(urls["archive"](pk))
    assert r.status_code == 200, r.text[:500]
    assert "archived_at" in r.json()

    r = admin["client"].get(urls["archived"])
    assert r.status_code == 200, r.text[:500]
    rows = r.json()
    assert isinstance(rows, list)
    assert {row["id"] for row in rows} >= {pk}

    r = admin["client"].get(urls["archived_detail"](pk))
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    require_keys(body, ARCHIVED_DETAIL_KEYS, "GET archived-cycles/<pk>/")
    assert body["archived_at"] is not None

    r = admin["client"].delete(urls["archive"](pk))
    assert r.status_code == 204, r.text[:500]


def test_archive_active_cycle_rejected(admin):
    urls = cycle_urls(admin)
    start = datetime.now(timezone.utc).date().isoformat()
    end = (datetime.now(timezone.utc) + timedelta(days=30)).date().isoformat()
    r = admin["client"].post(
        urls["cycles"],
        json={"name": "Active", "start_date": start, "end_date": end},
    )
    assert r.status_code == 201, r.text[:500]
    r = admin["client"].post(urls["archive"](r.json()["id"]))
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"error": "Only completed cycles can be archived"}


# -- create with a writable deleted_at (soft-delete on create) ---------------
#
# ``CycleWriteSerializer`` writes ``deleted_at`` (it is not in
# ``read_only_fields``, app/serializers/cycle.py:38-43), so a POST carrying a
# valid ``deleted_at`` stores the row already soft-deleted. Django's create
# (app/views/cycle/base.py:270-329) then re-reads through the live
# ``SoftDeletionManager`` queryset, gets ``None``, and
# ``user_timezone_converter(None, ...)`` (utils/timezone_converter.py:17-39)
# does ``list(None)`` -> TypeError -> generic 500, before ``model_activity``
# is enqueued. Rust parsed the value but the create INSERT bound ``deleted_at``
# as a hard-coded NULL, so the row went in live, the re-read found it, and the
# handler answered 201 and enqueued ``model_activity``.

MODEL_ACTIVITY_TASK = "pi_dash.bgtasks.webhook_task.model_activity"


def _celery_depth():
    """Django lane: depth of the ``celery`` list on the AMQP redis broker.

    ``None`` when ``AMQP_URL`` is not a redis URL (nothing observable), the
    same guard ``auth_oauth/test_github_side_effects.py`` uses.
    """
    url = os.environ.get("AMQP_URL") or os.environ.get("CELERY_BROKER_URL")
    if not url or not url.startswith("redis"):
        return None
    import redis

    return redis.Redis.from_url(url).llen("celery")


def _rust_queue_count(conn):
    """Rust lane: rows in ``rust_job_queue`` for the given Celery task.

    The table is created by the Rust server's boot (``queue::ensure_schema``),
    so on a Django-only database it is absent and counts as 0.
    """
    with conn.cursor() as cur:
        cur.execute("SELECT to_regclass('public.rust_job_queue')")
        if cur.fetchone()[0] is None:
            return 0
        cur.execute(
            "SELECT count(*) FROM rust_job_queue WHERE task = %s",
            (MODEL_ACTIVITY_TASK,),
        )
        return cur.fetchone()[0]


def _rust_queue_rows_for_model(conn, model_id):
    with conn.cursor() as cur:
        cur.execute("SELECT to_regclass('public.rust_job_queue')")
        if cur.fetchone()[0] is None:
            return 0
        cur.execute(
            "SELECT count(*) FROM rust_job_queue"
            " WHERE task = %s AND kwargs->>'model_id' = %s",
            (MODEL_ACTIVITY_TASK, model_id),
        )
        return cur.fetchone()[0]


def test_cycle_create_with_deleted_at_500s_and_no_enqueue(admin):
    urls = cycle_urls(admin)
    conn = admin["seed"].conn
    name = f"Q64-{uuid.uuid4().hex[:8]}"
    before_celery = _celery_depth()
    before_rust = _rust_queue_count(conn)

    r = admin["client"].post(
        urls["cycles"],
        json={"name": name, "deleted_at": "2026-01-01T00:00:00Z"},
    )
    # Django: the soft-deleted row makes the live re-read return None and
    # user_timezone_converter blows up -> generic 500.
    assert r.status_code == 500, r.text[:500]
    assert r.json() == GENERIC_500

    # The row is persisted -- already soft-deleted -- in the base table
    # (read the table directly: the live cycle view hides it).
    with conn.cursor() as cur:
        cur.execute(
            "SELECT id, deleted_at FROM cycles"
            " WHERE project_id = %s AND name = %s",
            (admin["project"]["id"], name),
        )
        rows = cur.fetchall()
    assert len(rows) == 1, f"expected exactly the soft-deleted row, got {rows}"
    row_id, deleted_at = rows[0]
    assert deleted_at is not None, "the created row must be stored soft-deleted"

    # No side effect on either lane: model_activity must not be enqueued.
    assert _rust_queue_rows_for_model(conn, str(row_id)) == 0, (
        "model_activity must not be enqueued for a soft-deleted create"
    )
    after_rust = _rust_queue_count(conn)
    assert after_rust == before_rust, (
        f"rust_job_queue grew: {before_rust} -> {after_rust}"
    )
    if before_celery is not None:
        after_celery = _celery_depth()
        assert after_celery == before_celery, (
            f"celery broker depth changed: {before_celery} -> {after_celery}"
        )


def test_cycle_issues_state_id_in_filter(admin):
    # PIDASHCONV-1317: the cycle-issues list runs the same rich-filter
    # compiler; UUID lists must bind typed (pre-fix Rust 500d).
    urls = cycle_urls(admin)
    ws_id, project_id = admin["workspace"]["id"], admin["project"]["id"]
    state_b = admin["seed"].state(
        project_id, ws_id, name="Second state", group="started")
    cycle = create_cycle_for(admin, name="Filter cycle")
    issue_a = create_issue_for(admin, name="Cycle filter A")
    issue_b = create_issue_for(admin, name="Cycle filter B")
    with admin["seed"].conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET state_id = %s WHERE id = %s",
            (state_b, issue_b["id"]),
        )
    try:
        for issue in (issue_a, issue_b):
            r = admin["client"].post(
                urls["cycle_issues"](cycle["id"]),
                json={"issues": [issue["id"]]},
            )
            assert r.status_code == 201, r.text[:500]
        r = admin["client"].get(
            urls["cycle_issues"](cycle["id"]),
            params={"filters": json.dumps(
                {"and": [{"state_id__in": admin["state"]["id"]}]})},
        )
        assert r.status_code == 200, r.text[:500]
        assert {row["id"] for row in r.json()["results"]} == {issue_a["id"]}
        r = admin["client"].get(
            urls["cycle_issues"](cycle["id"]),
            params={"filters": json.dumps({"and": [{"state_id__in": (
                f"{admin['state']['id']},{state_b}")}]})},
        )
        assert r.status_code == 200, r.text[:500]
        assert {row["id"] for row in r.json()["results"]} == {
            issue_a["id"], issue_b["id"]}
    finally:
        for issue in (issue_a, issue_b):
            admin["client"].delete(
                urls["cycle_issue_detail"](cycle["id"], issue["id"]))
