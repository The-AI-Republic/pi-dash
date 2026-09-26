"""Shape + value pins for the 13 analytic paths (``app/urls/analytic.py``).

Seeded world (see ``conftest.seed``): workspace ``an-ws`` with project AN
(3 issues: high/5pts + urgent/3pts in backlog, medium/8pts completed now),
one saved view AV1 (``query={"workspace__slug": "an-ws"}``,
``query_dict={"x_axis": "priority", "y_axis": "issue_count"}``).

Every endpoint gets a shape assertion; value pins use the seed so the
Rust port must reproduce the same aggregations, not just the keys.
"""

from datetime import datetime, timezone


def test_analytics_priority_shape(clients, seed):
    resp = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/analytics/?x_axis=priority&y_axis=issue_count"
    )
    assert resp.status_code == 200
    body = resp.json()
    assert body["total"] == 3
    assert body["distribution"] == {
        "high": [{"dimension": "high", "count": 1}],
        "medium": [{"dimension": "medium", "count": 1}],
        "urgent": [{"dimension": "urgent", "count": 1}],
    }
    assert body["extras"] == {
        "state_details": {},
        "assignee_details": {},
        "label_details": {},
        "cycle_details": {},
        "module_details": {},
    }


def test_analytics_estimate_axis(clients, seed):
    # estimate sums join estimate_point (NULL here) — keys still pinned.
    resp = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/analytics/?x_axis=priority&y_axis=estimate"
    )
    assert resp.status_code == 200
    body = resp.json()
    assert body["total"] == 3
    assert set(body["distribution"]) == {"high", "medium", "urgent"}
    for buckets in body["distribution"].values():
        assert len(buckets) == 1
        assert set(buckets[0]) == {"dimension", "estimate"}


def test_analytics_state_extras(clients, seed):
    resp = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/analytics/?x_axis=state_id&y_axis=issue_count"
    )
    assert resp.status_code == 200
    body = resp.json()
    assert body["total"] == 3
    assert set(body["distribution"]) == {seed["state_backlog"], seed["state_done"]}
    assert body["distribution"][seed["state_backlog"]] == [
        {"dimension": seed["state_backlog"], "count": 2}
    ]
    details = body["extras"]["state_details"]
    assert {d["state__name"] for d in details} == {"AN Backlog", "AN Done"}
    for detail in details:
        assert set(detail) == {"state_id", "state__name", "state__color"}


def test_analytics_validation_errors(clients, seed):
    admin = clients["admin"]
    ws = seed["ws_slug"]
    resp = admin.get(f"/api/workspaces/{ws}/analytics/?x_axis=nope&y_axis=issue_count")
    assert resp.status_code == 400
    assert resp.json() == {
        "error": "x-axis and y-axis dimensions are required and the values should be valid"
    }
    resp = admin.get(
        f"/api/workspaces/{ws}/analytics/?x_axis=priority&y_axis=issue_count&segment=priority"
    )
    assert resp.status_code == 400
    assert resp.json() == {
        "error": "Both segment and x axis cannot be same and segment should be valid"
    }


def test_analytic_view_crud(clients, seed):
    admin = clients["admin"]
    ws = seed["ws_slug"]
    listing = admin.get(f"/api/workspaces/{ws}/analytic-view/")
    assert listing.status_code == 200
    assert isinstance(listing.json(), list)
    assert seed["view"] in {v["id"] for v in listing.json()}

    created = admin.post(
        f"/api/workspaces/{ws}/analytic-view/",
        json={"name": "AV-CRUD", "description": "roundtrip",
              "query_dict": {"priority": "high"}},
    )
    assert created.status_code == 201
    view = created.json()
    assert view["name"] == "AV-CRUD"
    assert view["query_dict"] == {"priority": "high"}
    # issue_filters POST translation is part of the contract.
    assert view["query"] == {"priority__in": "high"}
    assert view["workspace"] == seed["ws"]
    vid = view["id"]

    detail = admin.get(f"/api/workspaces/{ws}/analytic-view/{vid}/")
    assert detail.status_code == 200
    assert detail.json()["id"] == vid

    patched = admin.patch(
        f"/api/workspaces/{ws}/analytic-view/{vid}/", json={"name": "AV-CRUD2"}
    )
    assert patched.status_code == 200
    assert patched.json()["name"] == "AV-CRUD2"

    deleted = admin.delete(f"/api/workspaces/{ws}/analytic-view/{vid}/")
    assert deleted.status_code == 204
    assert admin.get(f"/api/workspaces/{ws}/analytic-view/{vid}/").status_code == 404


def test_saved_analytic_shape(clients, seed):
    resp = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/saved-analytic-view/{seed['view']}/"
    )
    assert resp.status_code == 200
    body = resp.json()
    assert body["total"] == 3
    assert body["distribution"] == {
        "high": [{"dimension": "high", "count": 1}],
        "medium": [{"dimension": "medium", "count": 1}],
        "urgent": [{"dimension": "urgent", "count": 1}],
    }

    segmented = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/saved-analytic-view/{seed['view']}/"
        "?segment=state__group"
    )
    assert segmented.status_code == 200
    assert segmented.json()["total"] == 3
    assert isinstance(segmented.json()["distribution"], dict)


def test_export_analytics_post(clients, seed):
    resp = clients["admin"].post(
        f"/api/workspaces/{seed['ws_slug']}/export-analytics/",
        json={"x_axis": "priority", "y_axis": "issue_count"},
    )
    assert resp.status_code == 200
    assert resp.json() == {
        "message": (
            "Once the export is ready it will be emailed to you at an-admin@example.com"
        )
    }

    bad = clients["admin"].post(
        f"/api/workspaces/{seed['ws_slug']}/export-analytics/",
        json={"x_axis": "nope", "y_axis": "issue_count"},
    )
    assert bad.status_code == 400
    assert bad.json() == {
        "error": "x-axis and y-axis dimensions are required and the values should be valid"
    }


def test_default_analytics_values(clients, seed):
    resp = clients["admin"].get(f"/api/workspaces/{seed['ws_slug']}/default-analytics/")
    assert resp.status_code == 200
    body = resp.json()
    assert body["total_issues"] == 3
    assert body["total_issues_classified"] == [
        {"state_group": "backlog", "state_count": 2},
        {"state_group": "completed", "state_count": 1},
    ]
    assert body["open_issues"] == 2
    assert body["open_issues_classified"] == [
        {"state_group": "backlog", "state_count": 2}
    ]
    assert body["issue_completed_month_wise"] == [
        {"month": datetime.now(timezone.utc).month, "count": 1}
    ]
    assert body["most_issue_created_user"] == [
        {
            "created_by__first_name": "an_admin",
            "created_by__last_name": "User",
            "created_by__display_name": "an_admin",
            "created_by__id": seed["admin"],
            "count": 3,
            "created_by__avatar_url": "",
        }
    ]
    assert body["most_issue_closed_user"] == []
    assert body["pending_issue_user"] == [
        {
            "assignees__first_name": None,
            "assignees__last_name": None,
            "assignees__display_name": None,
            "assignees__id": None,
            "count": 2,
            "assignees__avatar_url": None,
        }
    ]
    assert body["open_estimate_sum"] == 8
    assert body["total_estimate_sum"] == 16


def test_project_stats_values(clients, seed):
    resp = clients["admin"].get(f"/api/workspaces/{seed['ws_slug']}/project-stats/")
    assert resp.status_code == 200
    body = resp.json()
    assert body == [
        {
            "id": seed["project"],
            "total_issues": 3,
            "completed_issues": 1,
            "total_cycles": 0,
            "total_modules": 0,
            "total_members": 3,
        }
    ]

    fields = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/project-stats/?fields=total_issues,completed_issues"
    )
    assert fields.status_code == 200
    assert fields.json() == [
        {"id": seed["project"], "total_issues": 3, "completed_issues": 1}
    ]


def test_advance_overview_values(clients, seed):
    resp = clients["admin"].get(f"/api/workspaces/{seed['ws_slug']}/advance-analytics/")
    assert resp.status_code == 200
    assert resp.json() == {
        "total_users": {"count": 3},
        "total_admins": {"count": 1},
        "total_members": {"count": 1},
        "total_guests": {"count": 1},
        "total_projects": {"count": 1},
        "total_work_items": {"count": 3},
        "total_cycles": {"count": 0},
        "total_intake": {"count": 0},
        "agent_run_input_tokens": {"count": 0},
        "agent_run_output_tokens": {"count": 0},
        "agent_run_total_tokens": {"count": 0},
    }


def test_advance_workitems_and_bad_tab(clients, seed):
    resp = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/advance-analytics/?tab=work-items"
    )
    assert resp.status_code == 200
    assert resp.json() == {
        "total_work_items": {"count": 3},
        "started_work_items": {"count": 0},
        "backlog_work_items": {"count": 2},
        "un_started_work_items": {"count": 0},
        "completed_work_items": {"count": 1},
    }

    bad = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/advance-analytics/?tab=nope"
    )
    assert bad.status_code == 400
    assert bad.json() == {"message": "Invalid tab"}


def test_advance_stats_shape(clients, seed):
    resp = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/advance-analytics-stats/"
    )
    assert resp.status_code == 200
    assert resp.json() == [
        {
            "project_id": seed["project"],
            "project__name": "Analytics Project",
            "cancelled_work_items": 0,
            "completed_work_items": 1,
            "backlog_work_items": 2,
            "un_started_work_items": 0,
            "started_work_items": 0,
        }
    ]


def test_advance_chart_projects(clients, seed):
    resp = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/advance-analytics-charts/"
    )
    assert resp.status_code == 200
    body = resp.json()
    assert {row["key"] for row in body} == {
        "work_items", "cycles", "modules", "intake", "members", "pages", "views",
    }
    for row in body:
        assert set(row) == {"key", "name", "count"}
    counts = {row["key"]: row["count"] for row in body}
    assert counts["work_items"] == 3
    assert counts["members"] == 3
    assert counts["cycles"] == 0


def test_advance_chart_workitems_monthly(clients, seed):
    resp = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/advance-analytics-charts/?type=work-items"
    )
    assert resp.status_code == 200
    body = resp.json()
    # The seed workspace is created this month, so exactly one bucket exists.
    month_key = datetime.now(timezone.utc).strftime("%Y-%m-01")
    assert body["data"] == [
        {
            "key": month_key,
            "name": month_key,
            "count": 3,
            "completed_issues": 1,
            "created_issues": 3,
        }
    ]
    assert body["schema"] == {
        "completed_issues": "completed_issues",
        "created_issues": "created_issues",
    }


def test_advance_chart_custom(clients, seed):
    resp = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/advance-analytics-charts/"
        "?type=custom-work-items&x_axis=PRIORITY"
    )
    assert resp.status_code == 200
    body = resp.json()
    assert body["data"] == [
        {"key": "high", "name": "high", "count": 1},
        {"key": "medium", "name": "medium", "count": 1},
        {"key": "urgent", "name": "urgent", "count": 1},
    ]
    assert body["schema"] == {}


def test_project_advance_overview(clients, seed):
    resp = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/projects/{seed['project']}/advance-analytics/"
    )
    assert resp.status_code == 200
    assert resp.json() == {
        "total_work_items": {"count": 3},
        "started_work_items": {"count": 0},
        "backlog_work_items": {"count": 2},
        "un_started_work_items": {"count": 0},
        "completed_work_items": {"count": 1},
    }

    # A cycle_id that matches nothing yields all-zero stats, not an error.
    empty = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/projects/{seed['project']}/advance-analytics/"
        "?cycle_id=00000000-0000-0000-0000-000000000000"
    )
    assert empty.status_code == 200
    assert empty.json() == {
        "total_work_items": {"count": 0},
        "started_work_items": {"count": 0},
        "backlog_work_items": {"count": 0},
        "un_started_work_items": {"count": 0},
        "completed_work_items": {"count": 0},
    }


def test_project_advance_stats(clients, seed):
    resp = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/projects/{seed['project']}/advance-analytics-stats/"
    )
    assert resp.status_code == 200
    # No assignees seeded: a single null-assignee bucket.
    assert resp.json() == [
        {
            "display_name": None,
            "assignee_id": None,
            "avatar_url": None,
            "cancelled_work_items": 0,
            "completed_work_items": 1,
            "backlog_work_items": 2,
            "un_started_work_items": 0,
            "started_work_items": 0,
        }
    ]


def test_project_advance_chart(clients, seed):
    resp = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/projects/{seed['project']}/"
        "advance-analytics-charts/?type=work-items"
    )
    assert resp.status_code == 200
    body = resp.json()
    month_key = datetime.now(timezone.utc).strftime("%Y-%m-01")
    assert body["data"] == [
        {
            "key": month_key,
            "name": month_key,
            "count": 3,
            "completed_issues": 1,
            "created_issues": 3,
        }
    ]
    assert body["schema"] == {
        "completed_issues": "completed_issues",
        "created_issues": "created_issues",
    }

    # The project chart view has no "projects" type: the default is rejected.
    default = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/projects/{seed['project']}/advance-analytics-charts/"
    )
    assert default.status_code == 400
    assert default.json() == {"message": "Invalid type"}
