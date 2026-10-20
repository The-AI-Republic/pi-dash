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

import uuid

import pytest

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


def test_intake_create_mints_sequence_from_counter(org):
    """PIDASHCONV-897: the minted sequence_id follows the issue_sequences
    counter (Django Issue.save), not MAX(issues.sequence_id). An anchor row
    inserted straight via SQL — fixed high sequence_id, no counter row,
    same as migration-created data — must not shift the minted value."""
    backlog = seed_d21.create_state(
        org.conn, workspace_id=org.workspace["id"], project_id=org.project["id"],
        name="Backlog", group="backlog",
    )
    org.conn.execute(
        """INSERT INTO issues (id, created_at, updated_at, name, description_json,
               priority, sequence_id, project_id, state_id,
               workspace_id, description_html, sort_order,
               is_draft, git_work_branch, workpad, complexity_score)
           VALUES (%s, now(), now(), 'SQL anchor', '{}', 'high', 900,
               %s, %s, %s, '', 65535, false, '', '', 0)""",
        (str(uuid.uuid4()), org.project["id"], backlog["id"], org.workspace["id"]),
    )
    body = org.create_intake_issue(name="counter minted")
    assert body["issue_detail"]["sequence_id"] == 1
    row = org.conn.execute(
        "SELECT sequence_id FROM issues WHERE id = %s", (body["issue"],)
    ).fetchone()
    assert row[0] == 1
    counter = org.conn.execute(
        "SELECT sequence FROM issue_sequences WHERE issue_id = %s AND deleted_at IS NULL",
        (body["issue"],),
    ).fetchone()
    assert counter[0] == 1


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


def test_intake_delete_pending_unmoved_control_204(org):
    # Control for the moved-issue case below: an intake item still pending in
    # project A deletes cleanly (204) and soft-deletes both rows.
    created = org.create_intake_issue(name="stays put")
    r = org.request("DELETE", f"{_base(org)}{created['issue']}/", "admin")
    assert r.status_code == 204
    assert r.text == ""
    issue_row = org.conn.execute(
        "SELECT deleted_at FROM issues WHERE id = %s", (created["issue"],)
    ).fetchone()
    assert issue_row is not None and issue_row[0] is not None
    intake_row = org.conn.execute(
        "SELECT deleted_at FROM intake_issues WHERE issue_id = %s", (created["issue"],)
    ).fetchone()
    assert intake_row is not None and intake_row[0] is not None


def test_intake_delete_after_issue_moved_to_other_project_500(org):
    # The intake row stays in project A while its issue moves to project B.
    # Django scopes its cascade issue fetch to (workspace, project, pk), so it
    # misses, `issue.created_by_id` raises AttributeError and the request 500s
    # with nothing written. An id-only fetch instead finds the moved row and
    # soft-deletes it (and the intake row) — 204. Bug: PIDASHCONV-1164.
    #
    # project A needs a non-triage default state so accepting the item moves
    # its issue out of triage; project B needs a non-triage target state for
    # the move.
    seed_d21.create_state(
        org.conn, workspace_id=org.workspace["id"], project_id=org.project["id"],
        name="Backlog", group="backlog", default=True,
    )
    seed_d21.create_state(
        org.conn, workspace_id=org.workspace["id"], project_id=org.project2["id"],
        name="Backlog", group="backlog",
    )
    created = org.create_intake_issue(name="moved away")
    issue_id = created["issue"]
    # Accept (issue leaves triage → A's default state), then set back to
    # pending: the intake row is pending again but the issue is non-triage.
    r = org.request("PATCH", f"{_base(org)}{issue_id}/", "admin", json={"status": 1})
    assert r.status_code == 200
    r = org.request("PATCH", f"{_base(org)}{issue_id}/", "admin", json={"status": 0})
    assert r.status_code == 200
    # Move the issue into project B.
    r = org.request(
        "POST",
        f"/api/v1/workspaces/{org.workspace['slug']}/projects/{org.project['id']}"
        f"/work-items/{issue_id}/move/",
        "admin",
        json={"project": org.project2["id"]},
    )
    assert r.status_code == 200, r.text
    assert r.json()["project"] == org.project2["id"]
    # Delete through project A: Django 500s, writing nothing.
    r = org.request("DELETE", f"{_base(org)}{issue_id}/", "admin")
    assert r.status_code == 500
    assert r.json() == {"error": "Something went wrong please try again later"}
    issue_row = org.conn.execute(
        "SELECT deleted_at FROM issues WHERE id = %s", (issue_id,)
    ).fetchone()
    assert issue_row is not None and issue_row[0] is None
    intake_row = org.conn.execute(
        "SELECT deleted_at FROM intake_issues WHERE issue_id = %s", (issue_id,)
    ).fetchone()
    assert intake_row is not None and intake_row[0] is None


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


# ---------------------------------------------------------------------------
# PIDASHCONV-1163: an unknown stored ``user_timezone`` is a 400, not a 500
# ---------------------------------------------------------------------------
# ``TimezoneMixin.initial`` (``api/views/base.py:42-47``) runs after
# authentication and the permission gate and calls
# ``zoneinfo.ZoneInfo(request.user.user_timezone)``. An unknown name raises
# ``ZoneInfoNotFoundError`` — a ``KeyError`` subclass — which
# ``BaseAPIView.handle_exception`` (``base.py:160-164``) answers
# ``400 {"error": "The required key does not exist."}``. An EMPTY zone 500s
# (``ZoneInfo('')`` raises ``ValueError``, not ``KeyError``) and a NULL zone
# 500s too, so only unknown, non-empty zones 400. Intake shares one preamble
# (``intake.rs::context``) across all five actions, so each is pinned.

KEY_ERROR_BODY = {"error": "The required key does not exist."}
SERVER_ERROR_BODY = {"error": "Something went wrong please try again later"}
BAD_ZONE = "Not/AZone"
# ``zoneinfo`` names are case-sensitive on a case-sensitive filesystem;
# live-probed against Django: ``america/new_york`` 400s like ``Not/AZone``.
WRONG_CASE_ZONE = "america/new_york"
EMPTY_ZONE = ""


def _set_user_timezone(org, user_id, zone):
    org.conn.execute(
        "UPDATE users SET user_timezone = %s WHERE id = %s", (zone, user_id)
    )


def _intake_actions(org, issue_id):
    """The five intake actions: name -> (method, path, json-payload-or-None)."""
    base = _base(org)
    detail = f"{base}{issue_id}/"
    return {
        "list": ("GET", base, None),
        "create": ("POST", base, {"issue": {"name": "tz probe"}}),
        "retrieve": ("GET", detail, None),
        "patch": ("PATCH", detail, {"issue": {"name": "tz patched"}}),
        "delete": ("DELETE", detail, None),
    }


def _call_intake(org, method, path, payload, role="admin"):
    kw = {"json": payload} if payload is not None else {}
    return org.request(method, path, role, **kw)


ACTIONS = ["list", "create", "retrieve", "patch", "delete"]


@pytest.mark.parametrize("action", ACTIONS)
def test_intake_unknown_zone_400(org, action):
    created = org.create_intake_issue(name="tz target")
    _set_user_timezone(org, org.admin["id"], BAD_ZONE)
    method, path, payload = _intake_actions(org, created["issue"])[action]
    r = _call_intake(org, method, path, payload)
    assert r.status_code == 400, r.text
    assert r.json() == KEY_ERROR_BODY


@pytest.mark.parametrize("action", ACTIONS)
def test_intake_wrong_case_zone_400(org, action):
    created = org.create_intake_issue(name="tz target")
    _set_user_timezone(org, org.admin["id"], WRONG_CASE_ZONE)
    method, path, payload = _intake_actions(org, created["issue"])[action]
    r = _call_intake(org, method, path, payload)
    assert r.status_code == 400, r.text
    assert r.json() == KEY_ERROR_BODY


@pytest.mark.parametrize("action", ACTIONS)
def test_intake_empty_zone_500(org, action):
    # ``ZoneInfo('')`` raises ``ValueError`` -> the generic 500 branch.
    created = org.create_intake_issue(name="tz target")
    _set_user_timezone(org, org.admin["id"], EMPTY_ZONE)
    method, path, payload = _intake_actions(org, created["issue"])[action]
    r = _call_intake(org, method, path, payload)
    assert r.status_code == 500, r.text
    assert r.json() == SERVER_ERROR_BODY


def test_intake_denied_caller_bad_zone_still_403(org):
    # The permission gate runs before ``TimezoneMixin.initial``: a denied
    # caller with an unknown zone 403s, never 400/500.
    org.create_intake_issue(name="tz denied")
    _set_user_timezone(org, org.outsider["id"], BAD_ZONE)
    r = _call_intake(org, "GET", _base(org), None, role="outsider")
    assert r.status_code == 403, r.text
