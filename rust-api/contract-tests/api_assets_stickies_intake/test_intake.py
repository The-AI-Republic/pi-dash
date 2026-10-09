"""Intake issues: list/create/retrieve/patch/delete.

``IntakeIssueListCreateAPIEndpoint`` (``GET``/``POST
/api/v1/workspaces/<slug>/projects/<project_id>/intake-issues/``) and
``IntakeIssueDetailAPIEndpoint`` (``GET``/``PATCH``/``DELETE .../<issue_id>/``,
where ``issue_id`` is the underlying *issue* id), gated by
``ProjectLitePermission`` — any active project member. Creation mints the
issue in the project's triage state; accepting (``status=1``) moves it to the
project's default state. Deleting a pending/rejected/snoozed/duplicate item
also deletes the underlying issue.
"""

from . import seed_d21

INTAKE_KEYS = {
    "created_at", "created_by", "deleted_at", "duplicate_to", "external_id",
    "external_source", "extra", "id", "inbox", "intake", "issue",
    "issue_detail", "project", "snoozed_till", "source", "source_email",
    "status", "updated_at", "updated_by", "workspace",
}

ISSUE_KEYS = {
    "id", "labels", "assignees", "state", "description", "created_at",
    "updated_at", "deleted_at", "point", "name", "description_json",
    "description_html", "description_stripped", "description_binary",
    "priority", "complexity_score", "start_date", "target_date",
    "sequence_id", "sort_order", "completed_at", "archived_at", "is_draft",
    "external_source", "external_id", "git_work_branch", "created_via",
    "agent_executor", "created_by", "updated_by", "project", "workspace",
    "parent", "estimate_point", "type", "assigned_pod",
}

LIST_KEYS = {
    "count", "extra_stats", "grouped_by", "next_cursor", "next_page_results",
    "prev_cursor", "prev_page_results", "results", "sub_grouped_by",
    "total_count", "total_pages", "total_results",
}


def _base(org, project_id=None):
    return (
        f"/api/v1/workspaces/{org.workspace['slug']}"
        f"/projects/{project_id or org.project['id']}/intake-issues/"
    )


def test_intake_create_shape(org):
    body = org.create_intake_issue(name="triage me", priority="high")
    assert set(body) == INTAKE_KEYS
    assert body["status"] == -2
    assert body["source"] == "IN_APP"
    assert body["inbox"] == body["intake"] == org.intake["id"]
    assert body["issue"] == body["issue_detail"]["id"]
    assert body["project"] == org.project["id"]
    assert body["workspace"] == org.workspace["id"]
    detail = body["issue_detail"]
    assert set(detail) == ISSUE_KEYS
    assert detail["name"] == "triage me"
    assert detail["priority"] == "high"
    assert detail["state"]["name"] == "Triage"
    assert detail["state"]["group"] == "triage"


def test_intake_list_shape(org):
    created = org.create_intake_issue(name="listed")
    r = org.request("GET", _base(org), "admin")
    assert r.status_code == 200
    body = r.json()
    assert set(body) == LIST_KEYS
    assert body["total_count"] == 1
    assert len(body["results"]) == 1
    assert set(body["results"][0]) == INTAKE_KEYS
    assert body["results"][0]["id"] == created["id"]


def test_intake_create_requires_name(org):
    r = org.request("POST", _base(org), "admin", json={"issue": {"priority": "low"}})
    assert r.status_code == 400
    assert r.json() == {"error": "Name is required"}


def test_intake_create_rejects_bad_priority(org):
    r = org.request(
        "POST", _base(org), "admin", json={"issue": {"name": "x", "priority": "yesterday"}}
    )
    assert r.status_code == 400
    assert r.json() == {"error": "Invalid priority"}


def test_intake_create_when_intake_disabled_400(org):
    org.conn.execute("DELETE FROM intakes WHERE id = %s", (org.intake["id"],))
    org.conn.execute(
        "UPDATE projects SET intake_view = false WHERE id = %s", (org.project["id"],)
    )
    r = org.request("POST", _base(org), "admin", json={"issue": {"name": "x"}})
    assert r.status_code == 400
    assert r.json() == {
        "error": "Intake is not enabled for this project enable it through the project's api"
    }
    r = org.request("GET", _base(org), "admin")
    assert r.json()["total_count"] == 0


def test_intake_retrieve_shape(org):
    created = org.create_intake_issue(name="fetch me")
    r = org.request("GET", f"{_base(org)}{created['issue']}/", "admin")
    assert r.status_code == 200
    assert r.json() == created


def test_intake_retrieve_unknown_404(org):
    r = org.request(
        "GET", f"{_base(org)}123e4567-e89b-12d3-a456-426614174000/", "admin"
    )
    assert r.status_code == 404


def test_intake_patch_issue_fields(org):
    created = org.create_intake_issue(name="before")
    r = org.request(
        "PATCH", f"{_base(org)}{created['issue']}/", "admin",
        json={"issue": {"name": "after", "priority": "urgent"}},
    )
    assert r.status_code == 200
    body = r.json()
    assert set(body) == INTAKE_KEYS
    assert body["issue_detail"]["name"] == "after"
    assert body["issue_detail"]["priority"] == "urgent"


def test_intake_accept_moves_issue_to_default_state(org):
    default = seed_d21.create_state(
        org.conn, workspace_id=org.workspace["id"], project_id=org.project["id"],
        name="Backlog", group="backlog", default=True,
    )
    created = org.create_intake_issue(name="accept me")
    r = org.request(
        "PATCH", f"{_base(org)}{created['issue']}/", "admin", json={"status": 1}
    )
    assert r.status_code == 200
    assert r.json()["status"] == 1
    r = org.request("GET", f"{_base(org)}{created['issue']}/", "admin")
    assert r.json()["issue_detail"]["state"]["id"] == default["id"]


def test_intake_accept_without_default_state_400(org):
    created = org.create_intake_issue(name="stuck")
    r = org.request(
        "PATCH", f"{_base(org)}{created['issue']}/", "admin", json={"status": 1}
    )
    assert r.status_code == 400
    assert r.json() == {
        "status": ["Cannot accept intake issue: No default state found for the project"]
    }


def test_intake_guest_non_creator_cannot_edit(org):
    created = org.create_intake_issue(name="guest target")
    r = org.request(
        "PATCH", f"{_base(org)}{created['issue']}/", "guest",
        json={"issue": {"name": "hijack"}},
    )
    assert r.status_code == 400
    assert r.json() == {"error": "You cannot edit intake work items"}


def test_intake_guest_creator_may_edit_name(org):
    created = org.create_intake_issue(role="guest", name="guest draft")
    r = org.request(
        "PATCH", f"{_base(org)}{created['issue']}/", "guest",
        json={"issue": {"name": "guest final"}},
    )
    assert r.status_code == 200
    assert r.json()["issue_detail"]["name"] == "guest final"


def test_intake_snooze_window_semantics(org):
    # NOTE (suspected upstream bug, ported as-is): the list filter keeps rows
    # with ``snoozed_till >= now`` and drops rows with a *past* ``snoozed_till``,
    # i.e. an expired snooze hides the item instead of resurfacing it.
    created = org.create_intake_issue(name="snooze me")
    r = org.request(
        "PATCH", f"{_base(org)}{created['issue']}/", "admin",
        json={"snoozed_till": "2030-01-01T00:00:00Z"},
    )
    assert r.status_code == 200
    assert r.json()["snoozed_till"] is not None
    r = org.request("GET", _base(org), "admin")
    assert r.json()["total_count"] == 1
    r = org.request(
        "PATCH", f"{_base(org)}{created['issue']}/", "admin",
        json={"snoozed_till": "2020-01-01T00:00:00Z"},
    )
    assert r.status_code == 200
    r = org.request("GET", _base(org), "admin")
    assert r.json()["total_count"] == 0


def test_intake_delete_by_creator_removes_issue(org):
    created = org.create_intake_issue(role="member", name="doomed")
    r = org.request("DELETE", f"{_base(org)}{created['issue']}/", "member")
    assert r.status_code == 204
    assert r.text == ""
    r = org.request("GET", f"{_base(org)}{created['issue']}/", "admin")
    assert r.status_code == 404
    # ``Issue.delete()`` is a soft delete: the row survives with deleted_at set.
    row = org.conn.execute(
        "SELECT deleted_at FROM issues WHERE id = %s", (created["issue"],)
    ).fetchone()
    assert row is not None and row[0] is not None


def test_intake_delete_by_non_admin_non_creator_403(org):
    created = org.create_intake_issue(role="admin", name="not yours")
    r = org.request("DELETE", f"{_base(org)}{created['issue']}/", "member")
    assert r.status_code == 403
    assert r.json() == {"error": "Only admin or creator can delete the work item"}


def test_intake_delete_accepted_item_keeps_issue(org):
    seed_d21.create_state(
        org.conn, workspace_id=org.workspace["id"], project_id=org.project["id"],
        name="Backlog", group="backlog", default=True,
    )
    created = org.create_intake_issue(name="accepted then gone")
    r = org.request(
        "PATCH", f"{_base(org)}{created['issue']}/", "admin", json={"status": 1}
    )
    assert r.status_code == 200
    r = org.request("DELETE", f"{_base(org)}{created['issue']}/", "admin")
    assert r.status_code == 204
    row = org.conn.execute(
        "SELECT id FROM issues WHERE id = %s", (created["issue"],)
    ).fetchone()
    assert row is not None


EXPANDED_USER_KEYS = {
    "id", "first_name", "last_name", "email", "avatar", "avatar_url",
    "display_name",
}


def test_intake_list_per_page_limits_results(org):
    # Regression cover (PIDASHCONV-794/797 class): a single-value query
    # param must not 400 the extractor. ?per_page=1 returns the newest
    # item only, with the Django cursor "1:1:0" for the next page.
    org.create_intake_issue(name="first")
    second = org.create_intake_issue(name="second")
    r = org.request("GET", _base(org), "admin", params={"per_page": "1"})
    assert r.status_code == 200
    body = r.json()
    assert set(body) == LIST_KEYS
    assert body["total_count"] == 2
    assert body["count"] == 1
    assert len(body["results"]) == 1
    assert body["results"][0]["id"] == second["id"]
    assert set(body["results"][0]) == INTAKE_KEYS
    assert body["next_page_results"] is True
    assert body["prev_page_results"] is False
    assert body["next_cursor"] == "1:1:0"


def test_intake_list_cursor_walks_pages(org):
    # The offset is cursor.offset * per_page, so the cursor is only
    # meaningful together with the per_page it was issued for.
    first = org.create_intake_issue(name="first")
    org.create_intake_issue(name="second")
    page1 = org.request(
        "GET", _base(org), "admin", params={"per_page": "1"}
    ).json()
    r = org.request(
        "GET", _base(org), "admin",
        params={"per_page": "1", "cursor": page1["next_cursor"]},
    )
    assert r.status_code == 200
    body = r.json()
    assert body["total_count"] == 2
    assert [row["id"] for row in body["results"]] == [first["id"]]
    assert set(body["results"][0]) == INTAKE_KEYS
    assert body["next_cursor"] == "1:2:0"
    assert body["prev_cursor"] == "1:0:1"
    assert body["next_page_results"] is False
    assert body["prev_page_results"] is True


def test_intake_list_fields_narrows_item_keys(org):
    created = org.create_intake_issue(name="narrowed")
    r = org.request("GET", _base(org), "admin", params={"fields": "id,status"})
    assert r.status_code == 200
    body = r.json()
    assert body["total_count"] == 1
    assert body["results"] == [{"id": created["id"], "status": created["status"]}]


def test_intake_list_fields_repeated_param_last_wins(org):
    # request.GET.get returns the LAST value, so ?fields=id&fields=status
    # behaves exactly as ?fields=status.
    org.create_intake_issue(name="last wins")
    r = org.request(
        "GET", _base(org), "admin", params=[("fields", "id"), ("fields", "status")]
    )
    assert r.status_code == 200
    assert r.json()["results"] == [{"status": -2}]
    single = org.request("GET", _base(org), "admin", params={"fields": "status"})
    assert r.json() == single.json()


def test_intake_list_expand_created_by(org):
    org.create_intake_issue(name="expanded")
    r = org.request("GET", _base(org), "admin", params={"expand": "created_by"})
    assert r.status_code == 200
    row = r.json()["results"][0]
    assert set(row) == INTAKE_KEYS
    assert set(row["created_by"]) == EXPANDED_USER_KEYS
    assert row["created_by"]["id"] == org.admin["id"]
    assert row["created_by"]["email"] == org.admin["email"]


def test_intake_retrieve_fields_narrows_keys(org):
    created = org.create_intake_issue(name="fetch narrowed")
    r = org.request(
        "GET", f"{_base(org)}{created['issue']}/", "admin",
        params={"fields": "id,status"},
    )
    assert r.status_code == 200
    assert r.json() == {"id": created["id"], "status": created["status"]}


def test_intake_retrieve_expand_created_by(org):
    created = org.create_intake_issue(name="fetch expanded")
    r = org.request(
        "GET", f"{_base(org)}{created['issue']}/", "admin",
        params={"expand": "created_by"},
    )
    assert r.status_code == 200
    body = r.json()
    assert set(body) == INTAKE_KEYS
    assert set(body["created_by"]) == EXPANDED_USER_KEYS
    assert body["created_by"]["id"] == org.admin["id"]
    assert body["created_by"]["email"] == org.admin["email"]
    rest = {k: v for k, v in body.items() if k != "created_by"}
    assert rest == {k: v for k, v in created.items() if k != "created_by"}


def test_intake_retrieve_ignores_per_page(org):
    # Retrieve never reads per_page, but the param must still deserialize.
    created = org.create_intake_issue(name="paged retrieve")
    r = org.request(
        "GET", f"{_base(org)}{created['issue']}/", "admin",
        params={"per_page": "1"},
    )
    assert r.status_code == 200
    assert r.json() == created


def test_intake_retrieve_ignores_cursor(org):
    created = org.create_intake_issue(name="cursor retrieve")
    r = org.request(
        "GET", f"{_base(org)}{created['issue']}/", "admin",
        params={"cursor": "1:1:0"},
    )
    assert r.status_code == 200
    assert r.json() == created
