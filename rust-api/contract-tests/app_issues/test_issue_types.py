"""Project work-item types (PIDASHCONV-1679; Django NEWFRONT-984).

Ported from ``apps/api/pi_dash/tests/contract/app/test_issue_types.py``
(8 tests: list roles, outsider refusal, empty project, admin create × 2,
blank-name refusal, member-create refusal) plus the probed parity arms:
exact denial bodies per role, the decorator bypass (project MEMBER who is
workspace ADMIN may create), deleted/unknown/foreign projects, identifier
rewrite, validation bytes, coercions, HTML edges, and the stored-NaN 500.

Unowned methods (PUT/PATCH/DELETE/...) proxy to Django by design, so no
405 test lives here: the suite runs against Rust with a dead upstream, so
every assertion below proves Rust-served bytes (pre-fix the whole file
fails at the proxy).
"""

import math
import uuid
from datetime import datetime, timezone

import httpx
import pytest

from _harness.auth import authed_client, forge_session_key, get_secret_key

TYPE_KEYS = ["id", "name", "description", "logo_props", "is_epic",
             "is_default", "is_active", "level"]

FORBIDDEN_DETAIL = {"detail": "You do not have permission to perform this action."}
FORBIDDEN_ERROR = {"error": "You don't have the required permissions."}
UNAUTHENTICATED = {"detail": "Authentication credentials were not provided."}
PROJECT_NOT_FOUND = {"error": "Project not found"}
PROJECT_NOT_FOUND_DETAIL = {"detail": "Project not found"}
UNSUPPORTED_TEXT = {"detail": 'Unsupported media type "text/plain" in request.'}

NOW = datetime.now(timezone.utc)


def _wipe_types(cur):
    cur.execute("DELETE FROM project_issue_types WHERE workspace_id IN "
                "(SELECT id FROM workspaces WHERE slug LIKE %s)", ("is-%",))
    cur.execute("DELETE FROM issue_types WHERE workspace_id IN "
                "(SELECT id FROM workspaces WHERE slug LIKE %s)", ("is-%",))
    cur.execute("DELETE FROM sessions WHERE user_id IN "
                "(SELECT id::text FROM users WHERE username LIKE %s)",
                ("is\\_ty%",))
    cur.execute("DELETE FROM project_members WHERE member_id IN "
                "(SELECT id FROM users WHERE username LIKE %s)"
                " OR project_id IN (SELECT id FROM projects"
                " WHERE identifier = %s)", ("is\\_ty%", "ISTD"))
    cur.execute("DELETE FROM workspace_members WHERE member_id IN "
                "(SELECT id FROM users WHERE username LIKE %s)", ("is\\_ty%",))
    cur.execute("DELETE FROM projects WHERE identifier = %s", ("ISTD",))
    cur.execute("DELETE FROM users WHERE username LIKE %s", ("is\\_ty%",))


@pytest.fixture(scope="module")
def ttypes(db_conn, seed):
    """Seed is- types/links + hybrid users + a deleted project (wiped after)."""
    from app_issues.conftest import (  # noqa: PLC0415
        _project_row, _user_row, PASSWORD_FIELD, PROJECT_COLUMNS,
        USER_COLUMNS)
    ids = {"ws": seed["ws"], "ws_slug": seed["ws_slug"],
           "project": seed["project"],
           "other_project": seed["other_project"],
           "admin": seed["admin"]}
    for key in ("bug", "epic", "task", "delt", "dead",
                "hybrid", "wsguest", "del_project"):
        ids[key] = str(uuid.uuid4())
    secret = get_secret_key()
    with db_conn.cursor() as cur:
        _wipe_types(cur)
        for key, name, email in (
            ("hybrid", "is_tyhybrid", "is-tyhybrid@example.com"),
            ("wsguest", "is_tywsguest", "is-tywsguest@example.com"),
        ):
            row = _user_row(ids[key], name, email, name)
            cur.execute("INSERT INTO users (%s) VALUES (%s)"
                        % (USER_COLUMNS, ",".join(["%s"] * 29)), row)
        for user_key, ws_role, proj_role in (
            ("hybrid", 20, 15),
            ("wsguest", 20, 5),
        ):
            cur.execute("INSERT INTO workspace_members (created_at, updated_at,"
                        " id, role, member_id, workspace_id, view_props,"
                        " default_props, issue_props, is_active,"
                        " explored_features, getting_started_checklist, tips)"
                        " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                        (NOW, NOW, str(uuid.uuid4()), ws_role, ids[user_key],
                         ids["ws"], "{}", "{}", "{}", True, "{}", "{}", "{}"))
            cur.execute("INSERT INTO project_members (created_at, updated_at,"
                        " id, role, member_id, project_id, workspace_id,"
                        " view_props, default_props, sort_order, preferences,"
                        " is_active)"
                        " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                        (NOW, NOW, str(uuid.uuid4()), proj_role, ids[user_key],
                         ids["project"], ids["ws"], "{}", "{}", 0.0, "{}",
                         True))
        cur.execute("INSERT INTO projects (%s) VALUES (%s)" % (
            PROJECT_COLUMNS, ",".join(["%s"] * 30)),
            _project_row(ids["del_project"], "Deleted Types Proj", "ISTD",
                         ids["ws"], True))
        cur.execute("UPDATE projects SET deleted_at = %s WHERE id = %s",
                    (NOW, ids["del_project"]))
        cur.execute("INSERT INTO project_members (created_at, updated_at, id,"
                    " role, member_id, project_id, workspace_id, view_props,"
                    " default_props, sort_order, preferences, is_active)"
                    " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                    (NOW, NOW, str(uuid.uuid4()), 20, ids["admin"],
                     ids["del_project"], ids["ws"], "{}", "{}", 0.0, "{}",
                     True))
        for key, name, desc, logo, epic, lvl in (
            ("bug", "T-Bug", "A defect", '{"icon":"bug"}', False, 3.5),
            ("epic", "T-Epic", "", "{}", True, 0.0),
            ("task", "T-Task", "", "{}", False, 0.0),
            ("delt", "T-Del", "", "{}", False, 0.0),
            ("dead", "T-Dead", "", "{}", False, 0.0),
        ):
            cur.execute("INSERT INTO issue_types (created_at, updated_at, id,"
                        " name, description, logo_props, created_by_id,"
                        " workspace_id, is_active, is_default, level, is_epic)"
                        " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                        (NOW, NOW, ids[key], name, desc, logo, ids["admin"],
                         ids["ws"], True, False, lvl, epic))
        cur.execute("UPDATE issue_types SET deleted_at = %s WHERE id = %s",
                    (NOW, ids["delt"]))
        for key, link_lvl in (("epic", 0), ("bug", 1), ("delt", 2),
                              ("dead", 3)):
            cur.execute("INSERT INTO project_issue_types (created_at,"
                        " updated_at, id, level, is_default, created_by_id,"
                        " issue_type_id, project_id, workspace_id)"
                        " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                        (NOW, NOW, str(uuid.uuid4()), link_lvl, False,
                         ids["admin"], ids[key], ids["project"], ids["ws"]))
        cur.execute("UPDATE project_issue_types SET deleted_at = %s"
                    " WHERE issue_type_id = %s", (NOW, ids["dead"]))
        sessions = {}
        for user_key in ("hybrid", "wsguest"):
            sessions[user_key] = forge_session_key(
                db_conn, user_id=ids[user_key],
                password_field=PASSWORD_FIELD, secret=secret)
    ids["sessions"] = sessions
    yield ids
    with db_conn.cursor() as cur:
        _wipe_types(cur)


@pytest.fixture(scope="module")
def extra_clients(base_url, ttypes):
    handles = {k: authed_client(base_url, v)
               for k, v in ttypes["sessions"].items()}
    yield handles
    for handle in handles.values():
        handle.close()


def _coll(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}/issue-types/"


def _live_links(cur, project_id):
    cur.execute("SELECT COUNT(*) FROM project_issue_types"
                " WHERE project_id = %s AND deleted_at IS NULL",
                (project_id,))
    return cur.fetchone()[0]


def _tag():
    return "T%s" % uuid.uuid4().hex[:8]


# GET: reads are open to every active project member (admin/member/guest).


def test_member_lists_linked_types(clients, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    resp = clients["member"].get(_coll(ws, pid))
    assert resp.status_code == 200
    rows = resp.json()
    # Ordered by link level then name; creations land at higher link
    # levels, so the seed pair always leads whatever this module added.
    assert [row["name"] for row in rows[:2]] == ["T-Epic", "T-Bug"]
    assert list(rows[0]) == TYPE_KEYS
    assert list(rows[1]) == TYPE_KEYS
    by_name = {row["name"]: row for row in rows}
    assert by_name["T-Epic"]["is_epic"] is True
    assert by_name["T-Epic"]["level"] == 0.0
    assert by_name["T-Bug"]["is_epic"] is False
    assert by_name["T-Bug"]["level"] == 3.5
    assert by_name["T-Bug"]["description"] == "A defect"
    assert by_name["T-Bug"]["logo_props"] == {"icon": "bug"}
    # Unlinked, soft-deleted, and dead-linked rows stay out.
    assert "T-Task" not in by_name
    assert "T-Del" not in by_name
    assert "T-Dead" not in by_name


def test_guest_lists_types(clients, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    resp = clients["guest"].get(_coll(ws, pid))
    assert resp.status_code == 200
    assert [row["name"] for row in resp.json()[:2]] == ["T-Epic", "T-Bug"]


def test_admin_lists_types(clients, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    resp = clients["admin"].get(_coll(ws, pid))
    assert resp.status_code == 200
    assert [row["name"] for row in resp.json()[:2]] == ["T-Epic", "T-Bug"]


def test_outsider_is_refused(clients, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    resp = clients["outsider"].get(_coll(ws, pid))
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN_DETAIL


def test_anonymous_list_is_refused(base_url, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    with httpx.Client(base_url=base_url, timeout=10,
                       follow_redirects=False) as anon:
        resp = anon.get(_coll(ws, pid))
    assert resp.status_code == 401
    assert resp.json() == UNAUTHENTICATED


def test_empty_project_lists_nothing(clients, seed, ttypes):
    resp = clients["member"].get(_coll(seed["ws_slug"], seed["project2"]))
    assert resp.status_code == 200
    assert resp.json() == []


def test_unknown_project_is_403(clients, ttypes):
    resp = clients["admin"].get(_coll(ttypes["ws_slug"], uuid.uuid4()))
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN_DETAIL


def test_other_workspace_project_is_403(clients, ttypes):
    # No membership row spans (slug, project), so the class gate fires
    # before the view body: 403, not the view's 404.
    resp = clients["outsider"].get(_coll(ttypes["ws_slug"], ttypes["other_project"]))
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN_DETAIL


def test_deleted_project_is_404(clients, ttypes):
    # The membership row lives on, so the gates pass and the view 404s.
    resp = clients["admin"].get(_coll(ttypes["ws_slug"], ttypes["del_project"]))
    assert resp.status_code == 404
    assert resp.json() == PROJECT_NOT_FOUND


def test_identifier_resolves(clients, seed, ttypes):
    resp = clients["admin"].get(_coll(seed["ws_slug"], "IS"))
    assert resp.status_code == 200
    assert [row["name"] for row in resp.json()[:2]] == ["T-Epic", "T-Bug"]


def test_unknown_identifier_is_404_detail(clients, seed, ttypes):
    resp = clients["admin"].get(_coll(seed["ws_slug"], "NOPE"))
    assert resp.status_code == 404
    assert resp.json() == PROJECT_NOT_FOUND_DETAIL


# POST: creation is admin-only (plus the workspace-admin bypass).


def test_admin_creates_and_links_type(clients, ttypes, db_conn):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    name = _tag()
    with db_conn.cursor() as cur:
        before = _live_links(cur, pid)
    resp = clients["admin"].post(
        _coll(ws, pid),
        json={"name": name, "description": "Time-boxed research",
              "is_epic": False})
    assert resp.status_code == 201
    body = resp.json()
    assert list(body) == TYPE_KEYS
    assert body["name"] == name
    assert body["description"] == "Time-boxed research"
    assert body["logo_props"] == {}
    assert body["is_epic"] is False
    assert body["is_default"] is False
    assert body["is_active"] is True
    assert body["level"] == 0.0
    with db_conn.cursor() as cur:
        cur.execute("SELECT workspace_id, created_by_id, updated_by_id"
                    " FROM issue_types WHERE id = %s", (body["id"],))
        row = cur.fetchone()
        assert (str(row[0]), str(row[1]), row[2]) == (ttypes["ws"],
                                                     ttypes["admin"], None)
        cur.execute("SELECT level FROM project_issue_types"
                    " WHERE project_id = %s AND issue_type_id = %s"
                    " AND deleted_at IS NULL", (pid, body["id"]))
        assert cur.fetchone()[0] == before


def test_admin_creates_epic_type(clients, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    resp = clients["admin"].post(
        _coll(ws, pid), json={"name": _tag(), "is_epic": True})
    assert resp.status_code == 201
    assert resp.json()["is_epic"] is True


def test_admin_create_full_shape(clients, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    resp = clients["admin"].post(
        _coll(ws, pid),
        json={"name": _tag(), "description": "d",
              "logo_props": {"a": 1}, "is_epic": True, "level": 2.5})
    assert resp.status_code == 201
    body = resp.json()
    assert (body["description"], body["logo_props"], body["is_epic"],
            body["level"]) == ("d", {"a": 1}, True, 2.5)


def test_blank_name_is_refused(clients, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    for name in ("   ", ""):
        resp = clients["admin"].post(_coll(ws, pid), json={"name": name})
        assert resp.status_code == 400
        assert resp.json() == {"name": ["This field may not be blank."]}


def test_member_cannot_create(clients, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    resp = clients["member"].post(_coll(ws, pid), json={"name": _tag()})
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN_ERROR


def test_guest_create_denied_detail(clients, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    resp = clients["guest"].post(_coll(ws, pid), json={"name": _tag()})
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN_DETAIL


def test_outsider_create_denied_detail(clients, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    resp = clients["outsider"].post(_coll(ws, pid), json={"name": _tag()})
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN_DETAIL


def test_anonymous_create_denied(base_url, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    with httpx.Client(base_url=base_url, timeout=10,
                       follow_redirects=False) as anon:
        resp = anon.post(_coll(ws, pid), json={"name": _tag()})
    assert resp.status_code == 401
    assert resp.json() == UNAUTHENTICATED


def test_hybrid_member_wsadmin_can_create(extra_clients, ttypes):
    # Project MEMBER + workspace ADMIN passes the decorator bypass.
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    resp = extra_clients["hybrid"].post(_coll(ws, pid), json={"name": _tag()})
    assert resp.status_code == 201


def test_wsguest_create_denied_detail(extra_clients, ttypes):
    # Project GUEST + workspace ADMIN still fails the class gate first.
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    resp = extra_clients["wsguest"].post(_coll(ws, pid), json={"name": _tag()})
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN_DETAIL


def test_deleted_project_post_is_404(clients, ttypes):
    resp = clients["admin"].post(_coll(ttypes["ws_slug"], ttypes["del_project"]),
                                 json={"name": _tag()})
    assert resp.status_code == 404
    assert resp.json() == PROJECT_NOT_FOUND


@pytest.mark.parametrize(("payload", "expected"), [
    ({}, {"name": ["This field is required."]}),
    ({"name": None}, {"name": ["This field may not be null."]}),
    ({"name": True}, {"name": ["Not a valid string."]}),
    ({"name": ["x"]}, {"name": ["Not a valid string."]}),
    ({"name": "x" * 256},
     {"name": ["Ensure this field has no more than 255 characters."]}),
    ({"name": "n", "description": None},
     {"description": ["This field may not be null."]}),
    ({"name": "n", "description": True},
     {"description": ["Not a valid string."]}),
    ({"name": "n", "logo_props": None},
     {"logo_props": ["This field may not be null."]}),
    ({"name": "n", "is_epic": ""},
     {"is_epic": ["Must be a valid boolean."]}),
    ({"name": "n", "is_epic": None},
     {"is_epic": ["This field may not be null."]}),
    ({"name": "n", "is_epic": "bogus"},
     {"is_epic": ["Must be a valid boolean."]}),
    ({"name": "n", "level": ""},
     {"level": ["A valid number is required."]}),
    ({"name": "n", "level": None},
     {"level": ["This field may not be null."]}),
    ({"name": "n", "level": [1]},
     {"level": ["A valid number is required."]}),
])
def test_validation_errors(clients, ttypes, payload, expected):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    if "name" in payload and payload["name"] == "n":
        payload = dict(payload, name=_tag())
    resp = clients["admin"].post(_coll(ws, pid), json=payload)
    assert resp.status_code == 400
    assert resp.json() == expected


def test_top_level_shapes(clients, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    resp = clients["admin"].post(
        _coll(ws, pid), content="null",
        headers={"Content-Type": "application/json"})
    assert resp.status_code == 400
    assert resp.json() == {"non_field_errors": ["No data provided"]}
    resp = clients["admin"].post(_coll(ws, pid), json=[1, 2])
    assert resp.status_code == 400
    assert resp.json() == {"non_field_errors": [
        "Invalid data. Expected a dictionary, but got list."]}
    resp = clients["admin"].post(_coll(ws, pid))
    assert resp.status_code == 400
    assert resp.json() == {"name": ["This field is required."]}


@pytest.mark.parametrize(("field", "sent", "stored"), [
    ("name", 123, "123"),
    ("name", 12.5, "12.5"),
    ("name", "  Sp  ", "Sp"),
    ("name", "é" * 255, "é" * 255),
    ("name", "  " + "x" * 254 + "  ", "x" * 254),
    ("description", 5, "5"),
    ("description", "  d  ", "d"),
    ("description", "", ""),
    ("logo_props", "abc", "abc"),
    ("logo_props", "", ""),
    ("logo_props", '{"a":1}', '{"a":1}'),
    ("logo_props", [1, "a"], [1, "a"]),
    ("logo_props", 7, 7),
    ("logo_props", True, True),
    ("is_epic", "yes", True),
    ("is_epic", 1, True),
    ("is_epic", 0.0, False),
    ("level", "2.5", 2.5),
    ("level", True, 1.0),
    ("level", "1_0", 10.0),
])
def test_coercions(clients, ttypes, field, sent, stored):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    payload = {"name": _tag()}
    payload[field] = sent
    resp = clients["admin"].post(_coll(ws, pid), json=payload)
    assert resp.status_code == 201
    assert resp.json()[field] == stored


def test_multikey_logo_order(clients, ttypes):
    # The 201 echoes the submitted key order (Django renders its
    # in-memory instance); a later GET shows PG jsonb order on both.
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    name = _tag()
    resp = clients["admin"].post(
        _coll(ws, pid),
        json={"name": name, "logo_props": {"bb": 2, "a": 1, "c": 3}})
    assert resp.status_code == 201
    assert '"logo_props":{"bb":2,"a":1,"c":3}' in resp.text
    rows = clients["admin"].get(_coll(ws, pid)).json()
    assert [row for row in rows if row["name"] == name][0]["logo_props"] == {
        "a": 1, "c": 3, "bb": 2}


def test_unknown_and_readonly_keys_dropped(clients, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    resp = clients["admin"].post(
        _coll(ws, pid),
        json={"name": _tag(), "zzz": 1, "is_default": True,
              "is_active": False, "id": str(uuid.uuid4())})
    assert resp.status_code == 201
    body = resp.json()
    assert list(body) == TYPE_KEYS
    assert body["is_default"] is False
    assert body["is_active"] is True


def test_duplicate_names_allowed(clients, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    name = _tag()
    first = clients["admin"].post(_coll(ws, pid), json={"name": name})
    second = clients["admin"].post(_coll(ws, pid), json={"name": name})
    assert (first.status_code, second.status_code) == (201, 201)
    assert first.json()["id"] != second.json()["id"]


def test_html_edges(clients, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    resp = clients["admin"].post(
        _coll(ws, pid), data={"name": _tag(), "is_epic": "", "level": ""})
    assert resp.status_code == 201
    assert (resp.json()["is_epic"], resp.json()["level"]) == (False, 0.0)
    resp = clients["admin"].post(
        _coll(ws, pid), data={"name": _tag(), "is_epic": "false"})
    assert resp.status_code == 201
    assert resp.json()["is_epic"] is False
    resp = clients["admin"].post(
        _coll(ws, pid), data={"name": _tag(), "logo_props": "{bad"})
    assert resp.status_code == 400
    assert resp.json() == {"logo_props": ["Value must be valid JSON."]}
    resp = clients["admin"].post(
        _coll(ws, pid), data={"name": _tag(), "logo_props": ""})
    assert resp.status_code == 400
    assert resp.json() == {"logo_props": ["Value must be valid JSON."]}
    resp = clients["admin"].post(
        _coll(ws, pid), data={"name": _tag(), "logo_props": '{"a":1}'})
    assert resp.status_code == 201
    assert resp.json()["logo_props"] == {"a": 1}
    resp = clients["admin"].post(_coll(ws, pid), data={"name": ""})
    assert resp.status_code == 400
    assert resp.json() == {"name": ["This field may not be blank."]}
    resp = clients["admin"].post(_coll(ws, pid), data={"level": "1"})
    assert resp.status_code == 400
    assert resp.json() == {"name": ["This field is required."]}
    # Uploads under scalar keys fail each field's own coercion.
    resp = clients["admin"].post(
        _coll(ws, pid), files={"name": ("n.txt", b"x")})
    assert resp.status_code == 400
    assert resp.json() == {"name": ["Not a valid string."]}
    for field, message in (
        ("description", "Not a valid string."),
        ("logo_props", "Value must be valid JSON."),
        ("is_epic", "Must be a valid boolean."),
        ("level", "A valid number is required."),
    ):
        resp = clients["admin"].post(
            _coll(ws, pid),
            files={field: ("f.txt", b"x"), "name": (None, _tag())})
        assert resp.status_code == 400
        assert resp.json() == {field: [message]}


def test_html_null_logo_is_payload_invalid(clients, ttypes):
    # HTML "null" parses to JSON null, passes validation (the raw string
    # is non-empty), and dies on the NOT NULL jsonb column: Django's
    # IntegrityError arm, with nothing written.
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    name = _tag()
    resp = clients["admin"].post(
        _coll(ws, pid), data={"name": name, "logo_props": "null"})
    assert resp.status_code == 400
    assert resp.json() == {"error": "The payload is not valid"}
    names = [row["name"]
             for row in clients["admin"].get(_coll(ws, pid)).json()]
    assert name not in names


def test_unsupported_media_type(clients, ttypes):
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    resp = clients["admin"].post(
        _coll(ws, pid), content="x", headers={"Content-Type": "text/plain"})
    assert resp.status_code == 415
    assert resp.json() == UNSUPPORTED_TEXT


def test_malformed_json(clients, ttypes):
    # Parser error texts are tier 3 (CPython messages no client depends
    # on): status + envelope + prefix, like the app_assets suite.
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    resp = clients["admin"].post(
        _coll(ws, pid), content="{oops",
        headers={"Content-Type": "application/json"})
    assert resp.status_code == 400
    body = resp.json()
    assert set(body) == {"detail"}
    assert body["detail"].startswith("JSON parse error - ")


def test_nan_level_is_500_and_stored(clients, ttypes, db_conn):
    # Django stores the row + link, then the strict renderer 500s; the
    # row is removed again so later GETs (and reruns) stay clean.
    ws, pid = ttypes["ws_slug"], ttypes["project"]
    name = _tag()
    type_id = None
    try:
        resp = clients["admin"].post(
            _coll(ws, pid), json={"name": name, "level": "nan"})
        assert resp.status_code == 500
        with db_conn.cursor() as cur:
            cur.execute("SELECT id, level FROM issue_types"
                        " WHERE name = %s AND workspace_id = %s",
                        (name, ttypes["ws"]))
            row = cur.fetchone()
            assert row is not None
            assert math.isnan(row[1])
            type_id = row[0]
            cur.execute("SELECT COUNT(*) FROM project_issue_types"
                        " WHERE project_id = %s AND issue_type_id = %s"
                        " AND deleted_at IS NULL", (pid, type_id))
            assert cur.fetchone()[0] == 1
    finally:
        if type_id is not None:
            with db_conn.cursor() as cur:
                cur.execute("DELETE FROM project_issue_types"
                            " WHERE issue_type_id = %s", (type_id,))
                cur.execute("DELETE FROM issue_types WHERE id = %s",
                            (type_id,))
