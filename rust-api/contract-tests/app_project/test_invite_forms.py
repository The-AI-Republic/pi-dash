"""Contract tests: form/multipart bodies on the app_project invite endpoints.

PIDASHCONV-1508: DRF's default parsers accept ``application/x-www-form-urlencoded``
and ``multipart/form-data`` on the invite/join/favorite/deploy-board writes, but
the Rust port 415d every non-JSON body before the handler ran. Every test below
runs unchanged against Django and against Rust with identical expectations.

Board PATCH form semantics (probed live on Django, mirrors DRF ``get_value``):
blank booleans and ``updated_by`` behave as absent, blank ``entity_identifier`` /
``deleted_at`` / ``created_by`` / ``intake`` arrive as null, and ``view_props``
strings JSON-decode (``Value must be valid JSON.`` on failure).
"""

import urllib.parse

from psycopg.rows import dict_row

FORM = {"Content-Type": "application/x-www-form-urlencoded"}

SERVER_ERROR = {"error": "Something went wrong please try again later"}
VALID_DETAIL = {"error": "Please provide valid detail"}


def _form(**fields):
    return urllib.parse.urlencode(fields)


def test_join_form_accepted_zero(world, db):
    # The issue's exact case: form accepted=0 accepts (raw truthiness,
    # PIDASHCONV-1377) and stores false.
    from _harness import seed as _seed
    client, _, ws, project = world.full_stack()
    newcomer = world.user("formjoin")
    inv = _seed.project_invite(world.conn, project["id"], ws["id"],
                               email=newcomer["email"])
    newcomer_client = world.client(newcomer)
    resp = newcomer_client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/join/{inv['id']}/",
        content=_form(email=newcomer["email"], accepted="0"), headers=FORM)
    assert resp.status_code == 200
    assert resp.json() == {"message": "Project Invitation Accepted"}
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT accepted, responded_at FROM project_member_invites WHERE id=%s",
            (str(inv["id"]),))
        row = cur.fetchone()
        assert row["accepted"] is False
        assert row["responded_at"] is not None
        cur.execute(
            "SELECT is_active FROM workspace_members WHERE workspace_id=%s AND member_id=%s",
            (str(ws["id"]), str(newcomer["id"])))
        assert cur.fetchone()["is_active"] is True
        cur.execute(
            "SELECT role FROM project_members WHERE project_id=%s AND member_id=%s",
            (str(project["id"]), str(newcomer["id"])))
        assert cur.fetchone()["role"] == 15


def test_join_form_accepted_false_string(world, db):
    from _harness import seed as _seed
    client, _, ws, project = world.full_stack()
    newcomer = world.user("formjoinf")
    inv = _seed.project_invite(world.conn, project["id"], ws["id"],
                               email=newcomer["email"])
    newcomer_client = world.client(newcomer)
    resp = newcomer_client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/join/{inv['id']}/",
        content=_form(email=newcomer["email"], accepted="False"), headers=FORM)
    assert resp.status_code == 200
    assert resp.json() == {"message": "Project Invitation Accepted"}
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT accepted FROM project_member_invites WHERE id=%s",
            (str(inv["id"]),))
        assert cur.fetchone()["accepted"] is False


def test_join_multipart_accept(world, db):
    from _harness import seed as _seed
    client, _, ws, project = world.full_stack()
    newcomer = world.user("mpjoin")
    inv = _seed.project_invite(world.conn, project["id"], ws["id"],
                               email=newcomer["email"])
    newcomer_client = world.client(newcomer)
    resp = newcomer_client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/join/{inv['id']}/",
        files={"email": (None, newcomer["email"]), "accepted": (None, "1")})
    assert resp.status_code == 200
    assert resp.json() == {"message": "Project Invitation Accepted"}
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT accepted FROM project_member_invites WHERE id=%s",
            (str(inv["id"]),))
        assert cur.fetchone()["accepted"] is True


def test_invite_create_form_string_500(world, db):
    # A form `emails` arrives as one string; iterating it raises like the
    # JSON truthy-non-list arm (500, nothing created).
    client, _, ws, project = world.full_stack()
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/invitations/",
        content="emails=x%40example.com", headers=FORM)
    assert resp.status_code == 500
    assert resp.json() == SERVER_ERROR
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT COUNT(*) AS n FROM project_member_invites WHERE project_id=%s",
            (str(project["id"]),))
        assert cur.fetchone()["n"] == 0


def test_invite_create_form_blank_400(world):
    client, _, ws, project = world.full_stack()
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/invitations/",
        content="emails=", headers=FORM)
    assert resp.status_code == 400
    assert resp.json() == {"error": "Emails are required"}


def test_user_invite_create_form_uuid_string_400(world):
    client, _, ws, project = world.full_stack()
    user, mclient = world.add_member(ws, project)
    resp = mclient.post(
        f"/api/users/me/workspaces/{ws['slug']}/projects/invitations/",
        content=_form(project_ids=str(project["id"])), headers=FORM)
    assert resp.status_code == 400
    assert resp.json() == VALID_DETAIL


def test_user_invite_create_form_blank_201(world):
    client, _, ws, project = world.full_stack()
    user, mclient = world.add_member(ws, project)
    resp = mclient.post(
        f"/api/users/me/workspaces/{ws['slug']}/projects/invitations/",
        content="project_ids=", headers=FORM)
    assert resp.status_code == 201
    assert resp.json() == {"message": "Projects joined successfully"}


def test_fav_create_form_204(world, db):
    client, _, ws, project = world.full_stack()
    resp = client.post(f"/api/workspaces/{ws['slug']}/user-favorite-projects/",
                       content=_form(project=str(project["id"])), headers=FORM)
    assert resp.status_code == 204
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT COUNT(*) AS n FROM user_favorites WHERE project_id=%s",
            (str(project["id"]),))
        assert cur.fetchone()["n"] == 1


def test_fav_create_form_blank_400(world):
    client, _, ws, project = world.full_stack()
    resp = client.post(f"/api/workspaces/{ws['slug']}/user-favorite-projects/",
                       content="project=", headers=FORM)
    assert resp.status_code == 400
    assert resp.json() == VALID_DETAIL


def test_board_create_form_flags(world, db):
    client, _, ws, project = world.full_stack()
    base = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-deploy-boards/"
    resp = client.post(
        base,
        content="is_votes_enabled=1&is_comments_enabled=True&is_reactions_enabled=0",
        headers=FORM)
    assert resp.status_code == 200
    created = resp.json()
    assert created["is_votes_enabled"] is True
    assert created["is_comments_enabled"] is True
    assert created["is_reactions_enabled"] is False
    assert created["view_props"] == {"list": True, "kanban": True, "calendar": True,
                                     "gantt": True, "spreadsheet": True}
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT is_votes_enabled, is_comments_enabled, is_reactions_enabled"
            " FROM deploy_boards WHERE id=%s",
            (created["id"],))
        row = cur.fetchone()
        assert (row["is_votes_enabled"], row["is_comments_enabled"],
                row["is_reactions_enabled"]) == (True, True, False)


def test_board_create_form_views_string(world, db):
    # `views` is a raw `.get`: a form string stores as a JSON string.
    client, _, ws, project = world.full_stack()
    base = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-deploy-boards/"
    resp = client.post(base, content="views=hello", headers=FORM)
    assert resp.status_code == 200
    assert resp.json()["view_props"] == "hello"
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT view_props FROM deploy_boards WHERE id=%s",
                    (resp.json()["id"],))
        assert cur.fetchone()["view_props"] == "hello"


def test_board_create_form_intake_500(world, db):
    # Any non-null `intake` fails the FK-descriptor assignment (ported bug);
    # the get_or_create row persists past the failure.
    import uuid
    client, _, ws, project = world.full_stack()
    base = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-deploy-boards/"
    resp = client.post(base, content=_form(intake=str(uuid.uuid4())), headers=FORM)
    assert resp.status_code == 500
    assert resp.json() == SERVER_ERROR
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT COUNT(*) AS n FROM deploy_boards WHERE project_id=%s AND deleted_at IS NULL",
            (str(project["id"]),))
        assert cur.fetchone()["n"] == 1


def test_board_patch_form_blank_bools_skip(world, db):
    # Blank HTML booleans behave as absent: preset flags stay set.
    client, _, ws, project = world.full_stack()
    base = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-deploy-boards/"
    board_id = client.post(base, json={}).json()["id"]
    url = f"{base}{board_id}/"
    resp = client.patch(url, json={"is_comments_enabled": True,
                                   "is_reactions_enabled": True,
                                   "is_votes_enabled": True,
                                   "is_disabled": True})
    assert resp.status_code == 200
    resp = client.patch(
        url,
        content=("is_comments_enabled=&is_reactions_enabled=&is_votes_enabled=&"
                 "is_activity_enabled=&is_disabled="),
        headers=FORM)
    assert resp.status_code == 200
    body = resp.json()
    for field in ("is_comments_enabled", "is_reactions_enabled", "is_votes_enabled",
                  "is_activity_enabled", "is_disabled"):
        assert body[field] is True, field
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT is_comments_enabled, is_reactions_enabled, is_votes_enabled,"
            " is_activity_enabled, is_disabled FROM deploy_boards WHERE id=%s",
            (board_id,))
        row = cur.fetchone()
        assert all(row[f] is True for f in row), dict(row)


def test_board_patch_form_blank_to_null(world, db):
    # Blank allow_null scalars arrive as None; blank `updated_by` converges
    # to the caller through the audit stamp.
    client, user, ws, project = world.full_stack()
    base = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-deploy-boards/"
    board_id = client.post(base, json={}).json()["id"]
    resp = client.patch(
        f"{base}{board_id}/",
        content="entity_identifier=&deleted_at=&created_by=&intake=&updated_by=",
        headers=FORM)
    assert resp.status_code == 200
    body = resp.json()
    assert body["entity_identifier"] is None
    assert body["created_by"] is None
    assert body["intake"] is None
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT entity_identifier, deleted_at, created_by_id, intake_id, updated_by_id"
            " FROM deploy_boards WHERE id=%s",
            (board_id,))
        row = cur.fetchone()
        assert row["entity_identifier"] is None
        assert row["deleted_at"] is None
        assert row["created_by_id"] is None
        assert row["intake_id"] is None
        assert str(row["updated_by_id"]) == str(user["id"])


def test_board_patch_form_view_props_json(world, db):
    # HTML `view_props` strings JSON-decode (DRF `JSONString`).
    client, _, ws, project = world.full_stack()
    base = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-deploy-boards/"
    board_id = client.post(base, json={}).json()["id"]
    resp = client.patch(f"{base}{board_id}/",
                        content=_form(view_props='{"a": 1}'), headers=FORM)
    assert resp.status_code == 200
    assert resp.json()["view_props"] == {"a": 1}
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT view_props FROM deploy_boards WHERE id=%s", (board_id,))
        assert cur.fetchone()["view_props"] == {"a": 1}


def test_board_patch_form_view_props_invalid(world, db):
    client, _, ws, project = world.full_stack()
    base = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-deploy-boards/"
    board_id = client.post(base, json={}).json()["id"]
    resp = client.patch(f"{base}{board_id}/", content="view_props=", headers=FORM)
    assert resp.status_code == 400
    assert resp.json() == {"view_props": ["Value must be valid JSON."]}
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT view_props FROM deploy_boards WHERE id=%s", (board_id,))
        assert cur.fetchone()["view_props"] == {"list": True, "kanban": True,
                                                "calendar": True, "gantt": True,
                                                "spreadsheet": True}


def test_board_patch_form_view_props_null_400(world, db):
    # HTML `view_props=null` decodes to None *after* `validate_empty_values`
    # ran (the `JSONString` is non-empty, so the null check never fires):
    # it validates, then dies at save with the not-null 400 (review probe).
    client, _, ws, project = world.full_stack()
    base = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-deploy-boards/"
    board_id = client.post(base, json={}).json()["id"]
    resp = client.patch(f"{base}{board_id}/", content="view_props=null", headers=FORM)
    assert resp.status_code == 400
    assert resp.json() == {"error": "The payload is not valid"}
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT view_props FROM deploy_boards WHERE id=%s", (board_id,))
        assert cur.fetchone()["view_props"] == {"list": True, "kanban": True,
                                                "calendar": True, "gantt": True,
                                                "spreadsheet": True}


def test_board_patch_form_view_props_null_yields_to_field_errors(world, db):
    # A decoded null contributes no field error of its own: sibling field
    # errors render alone and no write runs (review probe).
    client, _, ws, project = world.full_stack()
    base = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-deploy-boards/"
    board_id = client.post(base, json={}).json()["id"]
    resp = client.patch(f"{base}{board_id}/",
                        content="view_props=null&intake=garbage", headers=FORM)
    assert resp.status_code == 400
    assert resp.json() == {"intake": ["“garbage” is not a valid UUID."]}
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT view_props FROM deploy_boards WHERE id=%s", (board_id,))
        assert cur.fetchone()["view_props"] == {"list": True, "kanban": True,
                                                "calendar": True, "gantt": True,
                                                "spreadsheet": True}


def test_board_patch_form_normal_values(world, db):
    client, _, ws, project = world.full_stack()
    base = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-deploy-boards/"
    board_id = client.post(base, json={}).json()["id"]
    resp = client.patch(f"{base}{board_id}/",
                        content="is_votes_enabled=false&entity_name=page",
                        headers=FORM)
    assert resp.status_code == 200
    assert resp.json()["is_votes_enabled"] is False
    assert resp.json()["entity_name"] == "page"
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT is_votes_enabled, entity_name FROM deploy_boards WHERE id=%s",
            (board_id,))
        row = cur.fetchone()
        assert row["is_votes_enabled"] is False
        assert row["entity_name"] == "page"


def test_join_text_plain_415_exact(world):
    # The 415 arm is unchanged by the fix: pin its exact bytes on both.
    from _harness import seed as _seed
    client, user, ws, project = world.full_stack()
    inv = _seed.project_invite(world.conn, project["id"], ws["id"], email=user["email"])
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/join/{inv['id']}/",
        content=b'{"email": "x"}', headers={"Content-Type": "text/plain"})
    assert resp.status_code == 415
    assert resp.text == '{"detail":"Unsupported media type \\"text/plain\\" in request."}'
