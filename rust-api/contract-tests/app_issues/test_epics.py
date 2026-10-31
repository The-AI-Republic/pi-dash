"""Epic endpoints + type_id/is_epic issue reads (NEWFRONT-984).

Port of ``apps/api/pi_dash/tests/contract/app/test_epics.py`` to the
BASE_URL/DATABASE_URL contract harness: seeds straight into Postgres,
drives HTTP only. Runs against live Django and the Rust binary.

There is no Epic model: an epic is an issue whose type carries
``is_epic``. The epics views subclass the issue views, so reads, writes,
permissions and activities mirror the issues collection exactly; the
only epic-specific behavior is the boundary (epic-typed rows in,
everything else 404) and the forced epic type on create.

Each test gets fresh rows (function-scoped ``epics`` fixture) mirroring
the Django suite's per-test rollback; API-created rows are tracked and
removed in teardown so the session seed stays pristine.
"""

import uuid

import pytest

from .conftest import ISSUE_COLUMNS, PROJECT_COLUMNS, _issue_row, _project_row
from .test_issue_core import LIST_KEYS

# IssueSerializer detail key order, verified against live Django: the
# retrieve body is Meta.fields order (type_id/is_epic ride after
# cycle_id) plus the detail-only keys minus unannotated is_intake.
DETAIL_KEYS_ORDERED = [
    "id", "name", "state_id", "sort_order", "completed_at",
    "estimate_point", "priority", "complexity_score", "start_date",
    "target_date", "sequence_id", "project_id", "parent_id", "cycle_id",
    "type_id", "is_epic", "assigned_pod_id", "agent_executor",
    "module_ids", "label_ids", "assignee_ids", "sub_issues_count",
    "created_at", "updated_at", "created_by", "updated_by",
    "attachment_count", "link_count", "is_draft", "archived_at",
    "is_synced", "description_html", "is_subscribed", "agent_ticker",
    "agent_status", "relations_summary", "has_open_blockers",
]

# Flat issues/list/ serializer-branch key order (annotations SkipField
# out; expand= assignees appends the nested key), live Django.
FLAT_KEYS_ORDERED = [
    "id", "name", "state_id", "sort_order", "completed_at",
    "estimate_point", "priority", "complexity_score", "start_date",
    "target_date", "sequence_id", "project_id", "parent_id", "type_id",
    "is_epic", "assigned_pod_id", "agent_executor", "created_at",
    "updated_at", "created_by", "updated_by", "is_draft", "archived_at",
    "is_synced",
]

# Sub-issue row key order: type_id is a concrete column so it renders
# with the concrete block (after parent_id), live Django.
SUB_KEYS_ORDERED = [
    "id", "name", "state_id", "sort_order", "completed_at",
    "estimate_point", "priority", "start_date", "target_date",
    "sequence_id", "project_id", "parent_id", "type_id", "created_at",
    "updated_at", "created_by", "updated_by", "is_draft", "archived_at",
    "cycle_id", "link_count", "attachment_count", "sub_issues_count",
    "label_ids", "assignee_ids", "module_ids", "state_group",
]

NOT_FOUND = {"error": "The required object does not exist."}
ISSUE_NOT_FOUND = {"error": "Issue not found"}
FORBIDDEN = {"error": "You don't have the required permissions."}
GUEST_VIEW = {"error": "You are not allowed to view this issue"}
NO_EPIC_TYPE = {"error": "This project has no epic type, so no epic can be created."}
TYPE_LOCKED = {"error": "An epic's type cannot change through the epics collection."}

TYPE_COLUMNS = (
    "created_at, updated_at, id, name, description, logo_props,"
    " workspace_id, is_active, is_default, is_epic, level"
)

LINK_COLUMNS = (
    "created_at, updated_at, deleted_at, id, level, is_default,"
    " issue_type_id, project_id, workspace_id"
)


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


@pytest.fixture
def epics(db_conn, seed):
    """Fresh epic world per test: types + link + typed/untyped issues."""
    from datetime import datetime, timezone

    now = datetime.now(timezone.utc)
    ids = {
        "epic_type": str(uuid.uuid4()),
        "bug_type": str(uuid.uuid4()),
        "link": str(uuid.uuid4()),
        "epic": str(uuid.uuid4()),
        "plain": str(uuid.uuid4()),
        "bug": str(uuid.uuid4()),
        "parent": str(uuid.uuid4()),
        "child": str(uuid.uuid4()),
        "g_project": str(uuid.uuid4()),
        "g_state": str(uuid.uuid4()),
        "g_link": str(uuid.uuid4()),
        "g_epic": str(uuid.uuid4()),
    }
    ws, proj, admin, state = (
        seed["ws"], seed["project"], seed["admin"], seed["state_backlog"])
    created = []
    with db_conn.cursor() as cur:
        for key, name, is_epic in (("epic_type", "Epic", True),
                                   ("bug_type", "Bug", False)):
            cur.execute(
                "INSERT INTO issue_types (%s) VALUES (%s,TRUE,FALSE,%s,0.0)" % (
                    TYPE_COLUMNS, ",".join(["%s"] * 7), "%s"),
                (now, now, ids[key], name, "", "{}", ws, is_epic),
            )
        cur.execute(
            "INSERT INTO project_issue_types (%s) VALUES"
            " (%s,NULL,%s,0,FALSE,%s)" % (
                LINK_COLUMNS, ",".join(["%s"] * 2), "%s",
                ",".join(["%s"] * 3)),
            (now, now, ids["link"], ids["epic_type"], proj, ws),
        )
        rows = (
            ("epic", "Epic one", 101, "epic_type"),
            ("plain", "Plain one", 102, None),
            ("bug", "Bug one", 103, "bug_type"),
            ("parent", "Parent one", 104, None),
            ("child", "Child one", 105, "bug_type"),
        )
        for key, name, seq, type_key in rows:
            cur.execute(
                "INSERT INTO issues (%s) VALUES (%s)" % (
                    ISSUE_COLUMNS, ",".join(["%s"] * 19)),
                _issue_row(ids[key], name, "high", seq, admin, proj,
                           state, ws),
            )
            if type_key is not None:
                cur.execute(
                    "UPDATE issues SET type_id = %s WHERE id = %s",
                    (ids[type_key], ids[key]),
                )
        cur.execute(
            "UPDATE issues SET parent_id = %s WHERE id = %s",
            (ids["parent"], ids["child"]),
        )
        # Guest-scoped project (guest_view_all_features off): the guest
        # creator rule needs a project that hides others' rows.
        cur.execute(
            "INSERT INTO projects (%s) VALUES (%s)" % (
                PROJECT_COLUMNS, ",".join(["%s"] * 30)),
            _project_row(ids["g_project"], "Epic Guest Project", "ISG",
                         ws, False),
        )
        for user_key, role in (("admin", 20), ("member", 15), ("guest", 5)):
            cur.execute(
                "INSERT INTO project_members (created_at, updated_at, id, role,"
                " member_id, project_id, workspace_id, view_props,"
                " default_props, sort_order, preferences, is_active)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (now, now, str(uuid.uuid4()), role, seed[user_key],
                 ids["g_project"], ws, "{}", "{}", 0.0, "{}", True),
            )
        cur.execute(
            "INSERT INTO states (created_at, updated_at, id, name, description,"
            " color, slug, project_id, workspace_id, sequence, \"group\","
            " \"default\", is_triage)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (now, now, ids["g_state"], "ISG Backlog", "", "#000000",
             "isg-backlog", ids["g_project"], ws, 0.0, "backlog", True,
             False),
        )
        cur.execute(
            "INSERT INTO project_issue_types (%s) VALUES"
            " (%s,NULL,%s,0,FALSE,%s)" % (
                LINK_COLUMNS, ",".join(["%s"] * 2), "%s",
                ",".join(["%s"] * 3)),
            (now, now, ids["g_link"], ids["epic_type"], ids["g_project"],
             ws),
        )
        cur.execute(
            "INSERT INTO issues (%s) VALUES (%s)" % (
                ISSUE_COLUMNS, ",".join(["%s"] * 19)),
            _issue_row(ids["g_epic"], "Guest epic", "high", 1, admin,
                       ids["g_project"], ids["g_state"], ws),
        )
        cur.execute(
            "UPDATE issues SET type_id = %s WHERE id = %s",
            (ids["epic_type"], ids["g_epic"]),
        )
    world = dict(ids)
    world["created"] = created
    yield world

    with db_conn.cursor() as cur:
        gone = [ids["epic"], ids["plain"], ids["bug"], ids["parent"],
                ids["child"], ids["g_epic"]] + created
        cur.execute(
            "UPDATE issues SET parent_id = NULL WHERE id = ANY(%s)", (gone,))
        for iid in gone:
            cur.execute("DELETE FROM issue_sequences WHERE issue_id = %s", (iid,))
            cur.execute("DELETE FROM issues WHERE id = %s", (iid,))
        cur.execute(
            "DELETE FROM project_issue_types WHERE id IN (%s,%s)",
            (ids["link"], ids["g_link"]),
        )
        cur.execute(
            "DELETE FROM issue_types WHERE id IN (%s,%s)",
            (ids["epic_type"], ids["bug_type"]),
        )
        cur.execute(
            "DELETE FROM project_members WHERE project_id = %s",
            (ids["g_project"],),
        )
        cur.execute("DELETE FROM states WHERE id = %s", (ids["g_state"],))
        cur.execute("DELETE FROM projects WHERE id = %s", (ids["g_project"],))


def _track(world, issue_id):
    world["created"].append(issue_id)
    return issue_id


# -- create ---------------------------------------------------------------

def test_member_creates_epic_with_forced_type(clients, seed, epics):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = member.post(
        f"{_base(ws, pid)}/epics/",
        json={"name": "Shiny epic", "state_id": seed["state_backlog"]},
    )
    assert resp.status_code == 201
    issue_id = _track(epics, resp.json()["id"])
    detail = member.get(f"{_base(ws, pid)}/issues/{issue_id}/")
    assert detail.status_code == 200
    assert detail.json()["type_id"] == epics["epic_type"]
    assert detail.json()["is_epic"] is True
    assert detail.json()["name"] == "Shiny epic"


def test_create_without_project_epic_type_is_refused(clients, seed, epics):
    member = clients["member"]
    ws, pid2 = seed["ws_slug"], seed["project2"]
    resp = member.post(
        f"{_base(ws, pid2)}/epics/",
        json={"name": "Typeless epic", "state_id": seed["state2_backlog"]},
    )
    assert resp.status_code == 400
    assert resp.json() == NO_EPIC_TYPE


def test_guest_cannot_create(clients, seed, epics):
    guest = clients["guest"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = guest.post(
        f"{_base(ws, pid)}/epics/",
        json={"name": "Guest epic"},
    )
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN


def test_epic_create_response_shape(clients, seed, epics):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = member.post(
        f"{_base(ws, pid)}/epics/",
        json={"name": "Shaped epic", "state_id": seed["state_backlog"]},
    )
    assert resp.status_code == 201
    _track(epics, resp.json()["id"])
    # Same 27-key .values() body as the issues create (no type_id).
    assert set(resp.json()) == LIST_KEYS


# -- reads ----------------------------------------------------------------

def test_list_returns_only_epics(clients, seed, epics):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = member.get(f"{_base(ws, pid)}/epics/")
    assert resp.status_code == 200
    assert [row["id"] for row in resp.json()["results"]] == [epics["epic"]]


def test_detail_carries_epic_marker(clients, seed, epics):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = member.get(f"{_base(ws, pid)}/epics/{epics['epic']}/")
    assert resp.status_code == 200
    assert resp.json()["is_epic"] is True
    assert resp.json()["name"] == "Epic one"
    assert list(resp.json()) == DETAIL_KEYS_ORDERED


def test_plain_issue_is_not_found(clients, seed, epics):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = member.get(f"{_base(ws, pid)}/epics/{epics['plain']}/")
    assert resp.status_code == 404
    assert resp.json() == NOT_FOUND


def test_guest_lists_but_does_not_retrieve(clients, seed, epics):
    # Inherited verbatim from the issues detail: guests list, but the
    # creator check refuses another member's row. Runs on the
    # guest-scoped project (the main seed project shows guests all rows).
    guest = clients["guest"]
    ws, pid = seed["ws_slug"], epics["g_project"]
    listed = guest.get(f"{_base(ws, pid)}/epics/")
    retrieved = guest.get(f"{_base(ws, pid)}/epics/{epics['g_epic']}/")
    assert listed.status_code == 200
    assert retrieved.status_code == 403
    assert retrieved.json() == GUEST_VIEW


def test_outsider_is_refused(clients, seed, epics):
    outsider = clients["outsider"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = outsider.get(f"{_base(ws, pid)}/epics/")
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN


# -- update ---------------------------------------------------------------

def test_member_renames_epic(clients, seed, epics):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = member.patch(
        f"{_base(ws, pid)}/epics/{epics['epic']}/",
        json={"name": "Renamed epic"},
    )
    assert resp.status_code == 204
    detail = member.get(f"{_base(ws, pid)}/epics/{epics['epic']}/")
    assert detail.json()["name"] == "Renamed epic"


def test_type_change_is_refused(clients, seed, epics):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = member.patch(
        f"{_base(ws, pid)}/epics/{epics['epic']}/",
        json={"type": str(uuid.uuid4())},
    )
    assert resp.status_code == 400
    assert resp.json() == TYPE_LOCKED
    detail = member.get(f"{_base(ws, pid)}/epics/{epics['epic']}/")
    assert detail.json()["type_id"] == epics["epic_type"]


def test_update_plain_issue_is_not_found(clients, seed, epics):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = member.patch(
        f"{_base(ws, pid)}/epics/{epics['plain']}/",
        json={"name": "Hacked"},
    )
    assert resp.status_code == 404
    assert resp.json() == ISSUE_NOT_FOUND
    detail = member.get(f"{_base(ws, pid)}/issues/{epics['plain']}/")
    assert detail.json()["name"] == "Plain one"


# -- delete ---------------------------------------------------------------

def test_admin_deletes_epic(clients, seed, epics):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.delete(f"{_base(ws, pid)}/epics/{epics['epic']}/")
    assert resp.status_code == 204
    assert admin.get(f"{_base(ws, pid)}/issues/{epics['epic']}/").status_code == 404


def test_delete_plain_issue_is_not_found(clients, seed, epics):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.delete(f"{_base(ws, pid)}/epics/{epics['plain']}/")
    assert resp.status_code == 404
    assert resp.json() == NOT_FOUND
    assert admin.get(f"{_base(ws, pid)}/issues/{epics['plain']}/").status_code == 200


# -- type markers on the issues reads -------------------------------------

def test_epic_detail_carries_marker_and_type(clients, seed, epics):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = member.get(f"{_base(ws, pid)}/issues/{epics['epic']}/")
    assert resp.status_code == 200
    assert resp.json()["is_epic"] is True
    assert resp.json()["type_id"] == epics["epic_type"]
    assert list(resp.json()) == DETAIL_KEYS_ORDERED


def test_plain_detail_answers_untyped(clients, seed, epics):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = member.get(f"{_base(ws, pid)}/issues/{epics['plain']}/")
    assert resp.status_code == 200
    assert resp.json()["is_epic"] is False
    assert resp.json()["type_id"] is None


def test_typed_non_epic_detail_answers_typed_not_epic(clients, seed, epics):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = member.get(f"{_base(ws, pid)}/issues/{epics['bug']}/")
    assert resp.status_code == 200
    assert resp.json()["is_epic"] is False
    assert resp.json()["type_id"] == epics["bug_type"]


def test_flat_branch_carries_markers(clients, seed, epics):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = member.get(
        f"{_base(ws, pid)}/issues/list/?issues={epics['epic']}&expand=assignees")
    assert resp.status_code == 200
    (row,) = resp.json()
    assert list(row) == FLAT_KEYS_ORDERED + ["assignees"]
    assert row["type_id"] == epics["epic_type"]
    assert row["is_epic"] is True


def test_declared_marker_expand_clobbers_to_null(clients, seed, epics):
    # `type_id`/`is_epic` are declared serializer fields, so `expand=`
    # clobbers them via `getattr(instance, "<name>_id", None)` (null);
    # undeclared names would be ignored and keep their values.
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    for name in ("type_id", "is_epic"):
        resp = member.get(
            f"{_base(ws, pid)}/issues/{epics['epic']}/?expand={name}")
        assert resp.status_code == 200
        assert resp.json()[name] is None


def test_sub_issue_rows_carry_type_id(clients, seed, epics):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = member.get(f"{_base(ws, pid)}/issues/{epics['parent']}/sub-issues/")
    assert resp.status_code == 200
    rows = {row["id"]: row for row in resp.json()["sub_issues"]}
    assert rows[epics["child"]]["type_id"] == epics["bug_type"]
    assert list(rows[epics["child"]]) == SUB_KEYS_ORDERED


# -- work items -----------------------------------------------------------

def test_attach_and_list_children(clients, seed, epics):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    attached = member.post(
        f"{_base(ws, pid)}/epics/{epics['epic']}/issues/",
        json={"sub_issue_ids": [epics["plain"]]},
    )
    assert attached.status_code == 200
    listed = member.get(f"{_base(ws, pid)}/epics/{epics['epic']}/issues/")
    assert listed.status_code == 200
    assert [row["id"] for row in listed.json()["sub_issues"]] == [epics["plain"]]
    assert "type_id" in listed.json()["sub_issues"][0]


def test_children_of_plain_issue_are_not_found(clients, seed, epics):
    member = clients["member"]
    ws, pid = seed["ws_slug"], seed["project"]
    listed = member.get(f"{_base(ws, pid)}/epics/{epics['plain']}/issues/")
    attached = member.post(
        f"{_base(ws, pid)}/epics/{epics['plain']}/issues/",
        json={"sub_issue_ids": [epics["plain"]]},
    )
    assert listed.status_code == 404
    assert listed.json() == NOT_FOUND
    assert attached.status_code == 404
    assert attached.json() == NOT_FOUND
