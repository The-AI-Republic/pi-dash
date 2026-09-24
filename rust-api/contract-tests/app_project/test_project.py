"""Contract tests: project URL module (20 routes).

Every test pins the live Django wire shape: status code plus the exact
response key set. Value assertions only where the test controls the value.
"""

from conftest import assert_keys

LIST_KEYS = [
    "archived_at", "created_at", "created_by", "cycle_view",
    "guest_view_all_features", "id", "identifier", "inbox_view",
    "intake_count", "is_default", "issue_views_view", "logo_props",
    "member_role", "module_view", "name", "network", "page_view",
    "project_lead", "sort_order", "updated_at", "updated_by", "workspace",
]

DETAIL_KEYS = [
    "agent_default_interval_seconds", "agent_default_max_ticks",
    "agent_executor_options", "agent_review_default_interval_seconds",
    "agent_test_default_interval_seconds", "agent_ticking_enabled", "anchor",
    "archive_in", "archived_at", "base_branch", "close_in", "cover_image",
    "cover_image_asset", "cover_image_url", "created_at", "created_by",
    "cycle_view", "default_agent_executor", "default_assignee",
    "default_state", "deleted_at", "description", "description_html",
    "description_text", "emoji", "estimate", "external_id", "external_source",
    "guest_view_all_features", "icon_prop", "id", "identifier", "inbox_view",
    "intake_view", "is_default", "is_favorite", "is_issue_type_enabled",
    "is_time_tracking_enabled", "issue_views_view", "logo_props",
    "member_role", "members", "members_can_edit_states", "module_view",
    "name", "network", "next_work_item_sequence", "page_view",
    "project_lead", "repo_url", "sort_order", "timezone", "updated_at",
    "updated_by", "workspace",
]

INVITE_KEYS = [
    "accepted", "created_at", "created_by", "deleted_at", "email", "id",
    "message", "project", "responded_at", "role", "token", "updated_at",
    "updated_by", "workspace",
]


def test_list_projects_shape(world):
    client, _, ws, project = world.full_stack()
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/")
    assert resp.status_code == 200
    body = resp.json()
    assert isinstance(body, list) and len(body) == 1
    assert_keys(body[0], LIST_KEYS, "list")
    assert body[0]["id"] == project["id"]
    assert body[0]["member_role"] == 20


def test_list_detail_shape(world):
    client, user, ws, project = world.full_stack()
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/details/")
    assert resp.status_code == 200
    body = resp.json()
    assert isinstance(body, list) and len(body) == 1
    assert_keys(body[0], DETAIL_KEYS, "list_detail")
    assert body[0]["id"] == project["id"]
    assert body[0]["members"] == [str(user["id"])]


def test_retrieve_project_shape(world):
    client, _, ws, project = world.full_stack()
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/")
    assert resp.status_code == 200
    assert_keys(resp.json(), DETAIL_KEYS, "retrieve")


def test_retrieve_missing_project_404(world):
    client, _, ws, _ = world.full_stack()
    resp = client.get(
        f"/api/workspaces/{ws['slug']}/projects/00000000-0000-0000-0000-000000000000/")
    assert resp.status_code == 404
    assert resp.json() == {"error": "Project does not exist"}


def test_retrieve_archived_project_404(world):
    client, _, ws, project = world.full_stack()
    resp = client.post(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/archive/")
    assert resp.status_code == 200
    assert_keys(resp.json(), ["archived_at"], "archive")
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/")
    assert resp.status_code == 404


def test_create_project_shape_and_side_effects(world, db):
    import uuid
    from _harness import seed as _seed
    user = world.user("creator")
    ws = world.workspace(user, "createshape")
    world.ws_member(ws, user, 20)
    client = world.client(user)
    tag = uuid.uuid4().hex[:8].upper()
    resp = client.post(f"/api/workspaces/{ws['slug']}/projects/",
                       json={"name": "Shaped %s" % tag, "identifier": "S%s" % tag[:9]})
    assert resp.status_code == 201
    assert_keys(resp.json(), DETAIL_KEYS, "create")
    pid = resp.json()["id"]
    with db.cursor() as cur:
        cur.execute("SELECT COUNT(*) AS n FROM states WHERE project_id=%s", (pid,))
        assert cur.fetchone()["n"] == 8
        # The triage row carries group='triage' (is_triage stays False;
        # TriageStateManager filters on group).
        cur.execute(
            "SELECT COUNT(*) AS n FROM states WHERE project_id=%s AND \"group\"='triage'",
            (pid,))
        assert cur.fetchone()["n"] == 1
        cur.execute(
            "SELECT COUNT(*) AS n FROM states WHERE project_id=%s AND \"group\"<>'triage'",
            (pid,))
        assert cur.fetchone()["n"] == 7
        cur.execute(
            "SELECT role FROM project_members WHERE project_id=%s AND member_id=%s",
            (pid, str(user["id"])))
        assert cur.fetchone()["role"] == 20


def test_create_project_empty_data_400(world):
    client, _, ws, _ = world.full_stack()
    resp = client.post(f"/api/workspaces/{ws['slug']}/projects/", json={})
    assert resp.status_code == 400


def test_create_project_duplicate_identifier_400(world):
    client, _, ws, project = world.full_stack()
    resp = client.post(f"/api/workspaces/{ws['slug']}/projects/",
                       json={"name": "Dupe Other", "identifier": project["identifier"]})
    assert resp.status_code == 400


def test_partial_update_project(world):
    client, _, ws, project = world.full_stack()
    resp = client.patch(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/",
                        json={"name": "Renamed Project", "description": "new desc"})
    assert resp.status_code == 200
    assert_keys(resp.json(), DETAIL_KEYS, "partial_update")
    assert resp.json()["name"] == "Renamed Project"


def test_partial_update_archived_project_400(world):
    client, _, ws, project = world.full_stack()
    client.post(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/archive/")
    resp = client.patch(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/",
                        json={"name": "Nope"})
    assert resp.status_code == 400
    assert resp.json() == {"error": "Archived projects cannot be updated"}


def test_destroy_project(world, db):
    client, _, ws, _ = world.full_stack()
    # The first project per workspace is auto-default and cannot be deleted,
    # so delete a second one.
    second = world.project(client, ws["slug"], name="Deletable")
    assert second["is_default"] is False
    resp = client.delete(f"/api/workspaces/{ws['slug']}/projects/{second['id']}/")
    assert resp.status_code == 204
    # Deletes are soft: the row stays with deleted_at set.
    with db.cursor() as cur:
        cur.execute("SELECT deleted_at FROM projects WHERE id=%s", (str(second["id"]),))
        assert cur.fetchone()["deleted_at"] is not None


def test_destroy_default_project_400(world):
    client, _, ws, project = world.full_stack()
    assert project["is_default"] is True
    resp = client.delete(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/")
    assert resp.status_code == 400
    assert resp.json() == {"error": "Default project cannot be deleted"}


def test_full_update_put_400(world):
    # PUT maps to the inherited full update, which demands the read-only
    # deleted_at/workspace fields, so it always 400s. Pinned as-is.
    client, _, ws, project = world.full_stack()
    resp = client.put(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/",
                      json={"name": "Full Put", "identifier": project["identifier"]})
    assert resp.status_code == 400
    assert_keys(resp.json(), ["deleted_at", "workspace"], "put")


def test_identifiers_lookup(world):
    client, _, ws, project = world.full_stack()
    resp = client.get(f"/api/workspaces/{ws['slug']}/project-identifiers/",
                      params={"name": project["identifier"].lower()})
    assert resp.status_code == 200
    assert_keys(resp.json(), ["exists", "identifiers"], "identifiers")
    assert resp.json()["exists"] == 1


def test_identifiers_missing_name_400(world):
    client, _, ws, _ = world.full_stack()
    resp = client.get(f"/api/workspaces/{ws['slug']}/project-identifiers/")
    assert resp.status_code == 400
    assert resp.json() == {"error": "Name is required"}


def test_identifiers_delete_unknown_204(world):
    client, _, ws, _ = world.full_stack()
    resp = client.request(
        "DELETE", f"/api/workspaces/{ws['slug']}/project-identifiers/",
        json={"name": "NOPEZZZ"})
    assert resp.status_code == 204


def test_identifiers_delete_live_project_400(world):
    client, _, ws, project = world.full_stack()
    resp = client.request(
        "DELETE", f"/api/workspaces/{ws['slug']}/project-identifiers/",
        json={"name": project["identifier"]})
    assert resp.status_code == 400
    assert resp.json() == {"error": "Cannot delete an identifier of an existing project"}


MEMBER_ROLE_KEYS = ["created_at", "id", "member", "original_role", "project", "role"]

MEMBER_ADMIN_KEYS = [
    "comment", "created_at", "created_by", "default_props", "deleted_at",
    "id", "is_active", "member", "preferences", "project", "role",
    "sort_order", "updated_at", "updated_by", "view_props", "workspace",
]


def test_members_list_shape(world):
    client, user, ws, project = world.full_stack()
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/members/")
    assert resp.status_code == 200
    body = resp.json()
    assert isinstance(body, list) and len(body) == 1
    assert_keys(body[0], MEMBER_ROLE_KEYS, "members-list")
    assert body[0]["role"] == 20


def test_members_create_shape(world, db):
    from _harness import seed as _seed
    client, _, ws, project = world.full_stack()
    newcomer = world.user("newcomer")
    world.ws_member(ws, newcomer, 15)
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/members/",
        json={"members": [{"member_id": str(newcomer["id"]), "role": 15}]})
    assert resp.status_code == 201
    body = resp.json()
    assert isinstance(body, list) and len(body) == 1
    assert_keys(body[0], MEMBER_ROLE_KEYS, "members-create")
    assert body[0]["role"] == 15
    with db.cursor() as cur:
        cur.execute(
            "SELECT COUNT(*) AS n FROM project_user_properties "
            "WHERE project_id=%s AND user_id=%s",
            (str(project["id"]), str(newcomer["id"])))
        assert cur.fetchone()["n"] == 1


def test_members_create_empty_400(world):
    client, _, ws, project = world.full_stack()
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/members/",
        json={"members": []})
    assert resp.status_code == 400
    assert resp.json() == {"error": "At least one member is required"}


def test_member_retrieve_admin_shape(world):
    client, _, ws, project = world.full_stack()
    member_id = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/members/").json()[0]["id"]
    resp = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/members/{member_id}/")
    assert resp.status_code == 200
    assert_keys(resp.json(), MEMBER_ADMIN_KEYS, "member-retrieve-admin")


def test_member_retrieve_guest_shape(world):
    client, _, ws, project = world.full_stack()
    _, guest_client = world.add_member(ws, project, ws_role=5, proj_role=5, name="guestv")
    member_id = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/members/").json()[0]["id"]
    resp = guest_client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/members/{member_id}/")
    assert resp.status_code == 200
    # Guests receive the same role-serializer shape as the member list here.
    assert_keys(resp.json(), MEMBER_ROLE_KEYS, "member-retrieve-guest")


def test_member_partial_update_role(world):
    client, _, ws, project = world.full_stack()
    _, member_client = world.add_member(ws, project, ws_role=15, proj_role=15, name="upd")
    member_id = [m for m in client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/members/").json()
        if m["role"] == 15][0]["id"]
    resp = client.patch(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/members/{member_id}/",
        json={"role": 5})
    assert resp.status_code == 200
    assert resp.json()["role"] == 5


def test_member_destroy_and_leave(world, db):
    client, _, ws, project = world.full_stack()
    _, member_client = world.add_member(ws, project, ws_role=15, proj_role=15, name="leaver")
    member_id = [m for m in client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/members/").json()
        if m["role"] == 15][0]["id"]
    resp = member_client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/members/leave/")
    assert resp.status_code == 204
    with db.cursor() as cur:
        cur.execute("SELECT is_active FROM project_members WHERE id=%s", (member_id,))
        assert cur.fetchone()["is_active"] is False
    # Sole admin cannot leave: re-login as the admin and try
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/members/leave/")
    assert resp.status_code == 400


def test_member_me_shape(world):
    client, user, ws, project = world.full_stack()
    resp = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-members/me/")
    assert resp.status_code == 200
    assert_keys(resp.json(), MEMBER_ADMIN_KEYS, "member-me")


def test_project_views_update_204(world, db):
    client, _, ws, project = world.full_stack()
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-views/",
        json={"view_props": {"kanban": True}, "sort_order": 3})
    assert resp.status_code == 204
    with db.cursor() as cur:
        cur.execute(
            "SELECT sort_order FROM project_members WHERE project_id=%s",
            (str(project["id"]),))
        assert cur.fetchone()["sort_order"] == 3


def test_invitations_list_and_retrieve_shape(world):
    from _harness import seed as _seed
    client, _, ws, project = world.full_stack()
    inv = _seed.project_invite(world.conn, project["id"], ws["id"])
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/invitations/")
    assert resp.status_code == 200
    body = resp.json()
    assert isinstance(body, list) and len(body) == 1
    assert_keys(body[0], INVITE_KEYS, "invitations-list")
    resp = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/invitations/{inv['id']}/")
    assert resp.status_code == 200
    assert_keys(resp.json(), INVITE_KEYS, "invitations-retrieve")


def test_invitations_create_is_500_known_bug(world):
    # Known bug, pinned: the view calls .delay() on the bulk_create result
    # list instead of on the invitation task, so creation always 500s after
    # persisting the rows. The Rust port must reproduce this.
    client, _, ws, project = world.full_stack()
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/invitations/",
        json={"emails": [{"email": "bug@example.com", "role": 15}]})
    assert resp.status_code == 500
    assert resp.json() == {"error": "Something went wrong please try again later"}


def test_invitations_create_empty_400(world):
    client, _, ws, project = world.full_stack()
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/invitations/",
        json={"emails": []})
    assert resp.status_code == 400
    assert resp.json() == {"error": "Emails are required"}


def test_invitations_destroy_204(world, db):
    from _harness import seed as _seed
    client, _, ws, project = world.full_stack()
    inv = _seed.project_invite(world.conn, project["id"], ws["id"])
    resp = client.delete(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/invitations/{inv['id']}/")
    assert resp.status_code == 204


def test_user_invitations_list_shape(world):
    from _harness import seed as _seed
    client, user, ws, project = world.full_stack()
    _seed.project_invite(world.conn, project["id"], ws["id"], email=user["email"])
    resp = client.get(f"/api/users/me/workspaces/{ws['slug']}/projects/invitations/")
    assert resp.status_code == 200
    body = resp.json()
    assert isinstance(body, list) and len(body) == 1
    assert_keys(body[0], INVITE_KEYS, "user-invitations-list")


def test_user_invitations_join_public_project(world, db):
    client, user, ws, project = world.full_stack()
    with world.conn.cursor() as cur:
        cur.execute("UPDATE projects SET network=2 WHERE id=%s", (str(project["id"]),))
    world.conn.commit()
    joiner = world.user("joiner")
    world.ws_member(ws, joiner, 15)
    joiner_client = world.client(joiner)
    resp = joiner_client.post(
        f"/api/users/me/workspaces/{ws['slug']}/projects/invitations/",
        json={"project_ids": [str(project["id"])]})
    assert resp.status_code == 201
    assert resp.json() == {"message": "Projects joined successfully"}
    with db.cursor() as cur:
        cur.execute(
            "SELECT role FROM project_members WHERE project_id=%s AND member_id=%s",
            (str(project["id"]), str(joiner["id"])))
        assert cur.fetchone()["role"] == 15


def test_project_roles_shape(world):
    client, user, ws, project = world.full_stack()
    resp = client.get(f"/api/users/me/workspaces/{ws['slug']}/project-roles/")
    assert resp.status_code == 200
    assert resp.json() == {str(project["id"]): 20}


def test_join_get_shape(world):
    from _harness import seed as _seed
    client, _, ws, project = world.full_stack()
    inv = _seed.project_invite(world.conn, project["id"], ws["id"])
    resp = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/join/{inv['id']}/")
    assert resp.status_code == 200
    assert_keys(resp.json(), INVITE_KEYS, "join-get")


def test_join_accept_creates_memberships(world, db):
    from _harness import seed as _seed
    client, _, ws, project = world.full_stack()
    newcomer = world.user("invited")
    inv = _seed.project_invite(world.conn, project["id"], ws["id"],
                               email=newcomer["email"])
    newcomer_client = world.client(newcomer)
    resp = newcomer_client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/join/{inv['id']}/",
        json={"email": newcomer["email"], "accepted": True})
    assert resp.status_code == 200
    assert resp.json() == {"message": "Project Invitation Accepted"}
    with db.cursor() as cur:
        cur.execute(
            "SELECT is_active FROM workspace_members WHERE workspace_id=%s AND member_id=%s",
            (str(ws["id"]), str(newcomer["id"])))
        assert cur.fetchone()["is_active"] is True
        cur.execute(
            "SELECT role FROM project_members WHERE project_id=%s AND member_id=%s",
            (str(project["id"]), str(newcomer["id"])))
        assert cur.fetchone()["role"] == 15


def test_join_declined_message(world):
    from _harness import seed as _seed
    client, user, ws, project = world.full_stack()
    inv = _seed.project_invite(world.conn, project["id"], ws["id"], email=user["email"])
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/join/{inv['id']}/",
        json={"email": user["email"], "accepted": False})
    assert resp.status_code == 200
    assert resp.json() == {"message": "Project Invitation was not accepted"}


def test_join_wrong_email_403(world):
    from _harness import seed as _seed
    client, _, ws, project = world.full_stack()
    inv = _seed.project_invite(world.conn, project["id"], ws["id"])
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/join/{inv['id']}/",
        json={"email": "someone-else@example.com", "accepted": True})
    assert resp.status_code == 403


BOARD_NONE_KEYS = [
    "created_by", "deleted_at", "entity_identifier", "entity_name", "intake",
    "is_activity_enabled", "is_comments_enabled", "is_disabled",
    "is_reactions_enabled", "is_votes_enabled", "updated_by", "view_props",
]


def test_deploy_boards_empty_shape(world):
    client, _, ws, project = world.full_stack()
    resp = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-deploy-boards/")
    assert resp.status_code == 200
    assert_keys(resp.json(), BOARD_NONE_KEYS, "boards-empty")


def test_deploy_boards_crud(world, db):
    client, _, ws, project = world.full_stack()
    base = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/project-deploy-boards/"
    resp = client.post(base, json={"is_comments_enabled": True, "is_votes_enabled": True})
    assert resp.status_code in (200, 201)
    created = resp.json()
    assert created["is_comments_enabled"] is True
    assert created["is_votes_enabled"] is True
    assert created["entity_identifier"] == str(project["id"])
    board_id = created.get("id")
    resp = client.get(base)
    assert resp.status_code == 200
    assert resp.json()["entity_identifier"] == str(project["id"])
    if board_id:
        resp = client.get(f"{base}{board_id}/")
        assert resp.status_code == 200
        resp = client.patch(f"{base}{board_id}/", json={"is_reactions_enabled": True})
        assert resp.status_code == 200
        assert resp.json()["is_reactions_enabled"] is True
        resp = client.delete(f"{base}{board_id}/")
        assert resp.status_code == 204


def test_archive_unarchive_cycle(world, db):
    client, _, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/archive/"
    resp = client.post(url)
    assert resp.status_code == 200
    assert_keys(resp.json(), ["archived_at"], "archive")
    with db.cursor() as cur:
        cur.execute("SELECT archived_at FROM projects WHERE id=%s", (str(project["id"]),))
        assert cur.fetchone()["archived_at"] is not None
    resp = client.delete(url)
    assert resp.status_code == 204
    with db.cursor() as cur:
        cur.execute("SELECT archived_at FROM projects WHERE id=%s", (str(project["id"]),))
        assert cur.fetchone()["archived_at"] is None


def test_favorites_list_empty_is_500_known_bug(world):
    # Known bug, pinned: the favorites list queryset/serializer raises, so
    # the endpoint 500s even with no favorites. The Rust port must reproduce.
    client, _, ws, _ = world.full_stack()
    resp = client.get(f"/api/workspaces/{ws['slug']}/user-favorite-projects/")
    assert resp.status_code == 500
    assert resp.json() == {"error": "Something went wrong please try again later"}


def test_favorites_create_and_destroy(world, db):
    client, _, ws, project = world.full_stack()
    resp = client.post(f"/api/workspaces/{ws['slug']}/user-favorite-projects/",
                       json={"project": str(project["id"])})
    assert resp.status_code == 204
    with db.cursor() as cur:
        cur.execute(
            "SELECT COUNT(*) AS n FROM user_favorites WHERE project_id=%s",
            (str(project["id"]),))
        assert cur.fetchone()["n"] == 1
    resp = client.delete(
        f"/api/workspaces/{ws['slug']}/user-favorite-projects/{project['id']}/")
    assert resp.status_code == 204
    with db.cursor() as cur:
        cur.execute(
            "SELECT COUNT(*) AS n FROM user_favorites WHERE project_id=%s",
            (str(project["id"]),))
        assert cur.fetchone()["n"] == 0


def test_member_preference_get_and_patch(world):
    client, user, ws, project = world.full_stack()
    url = (f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
           f"preferences/member/{user['id']}/")
    resp = client.patch(url, json={"theme": "dark"})
    assert resp.status_code == 200
    assert_keys(resp.json(), ["preferences"], "preference-patch")
    resp = client.get(url)
    assert resp.status_code == 200
    assert_keys(resp.json(), ["preferences", "project_id", "member_id", "workspace_id"],
                "preference-get")
