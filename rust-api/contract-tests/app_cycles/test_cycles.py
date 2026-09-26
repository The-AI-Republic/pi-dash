"""Response-shape coverage for the D-27 cycle routes.

Every URL pattern in app/urls/cycle.py plus the workspace cycles endpoint in
app/urls/workspace.py gets a shape assertion against the live backend.
"""

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
