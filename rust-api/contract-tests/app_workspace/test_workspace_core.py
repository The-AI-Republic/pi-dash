# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Workspace core: CRUD, slug check, members, invitations, join, join requests.

Covers the core routes of app/urls/workspace.py: workspace-slug-check,
workspaces/ (list/create), workspaces/<slug>/ (retrieve/update/destroy),
members/ (+ detail + leave), workspace-members/me/, workspace-views/,
project-members/, users/last-visited-workspace/, invitations/ (+ detail +
join/), users/me/workspaces/invitations/ and both join-request routes.

Two frozen Django bugs (ported, see PR body): GET workspaces/ always 400s
(the list's allow_permission reads kwargs["slug"], which the collection
route has no kwarg for) and GET users/last-visited-workspace/ always 500s
(User has no last_workspace_id attribute).
"""

import uuid

import pytest

WS_CREATE_KEYS = {
    "background_color",
    "created_at",
    "created_by",
    "deleted_at",
    "id",
    "logo",
    "logo_asset",
    "logo_url",
    "name",
    "organization_size",
    "owner",
    "role",
    "slug",
    "timezone",
    "total_members",
    "updated_at",
    "updated_by",
}

WS_KEYS = WS_CREATE_KEYS - {"role"}

MEMBER_KEYS = {
    "company_role",
    "created_at",
    "created_by",
    "default_props",
    "deleted_at",
    "explored_features",
    "getting_started_checklist",
    "id",
    "is_active",
    "issue_props",
    "member",
    "role",
    "tips",
    "updated_at",
    "updated_by",
    "view_props",
    "workspace",
}

MEMBER_USER_KEYS = {
    "avatar",
    "avatar_url",
    "display_name",
    "email",
    "first_name",
    "id",
    "is_bot",
    "last_login_medium",
    "last_name",
}

MEMBERS_ME_KEYS = MEMBER_KEYS | {"draft_issue_count"}

INVITE_KEYS = {
    "accepted",
    "created_at",
    "created_by",
    "deleted_at",
    "email",
    "id",
    "invite_link",
    "message",
    "responded_at",
    "role",
    "token",
    "updated_at",
    "updated_by",
    "workspace",
}

MY_JOIN_REQUEST_KEYS = {
    "admin_email",
    "created_at",
    "id",
    "message",
    "requester",
    "responded_at",
    "status",
    "updated_at",
}

ADMIN_JOIN_REQUEST_KEYS = MY_JOIN_REQUEST_KEYS | {
    "created_by",
    "deleted_at",
    "responded_by",
    "role",
    "updated_by",
    "workspace",
}


@pytest.fixture
def space(world):
    db = world["db"]
    ws = db.make_workspace("Acme", "acme", world["user"]["id"])
    db.make_workspace_member(ws["id"], world["user"]["id"], role=20)
    return ws


@pytest.fixture
def member_api(world, space):
    """other@example.com as a role-15 member of the space workspace."""
    from _harness import user_client

    world["db"].make_workspace_member(space["id"], world["other"]["id"], role=15)
    with user_client(world["other_key"]) as client:
        yield client


# --- workspace-slug-check ---


def test_slug_check_free(user_api):
    res = user_api.get("/api/workspace-slug-check/", params={"slug": "free-slug"})
    assert res.status_code == 200
    assert res.json() == {"status": True}


def test_slug_check_taken(user_api, space):
    res = user_api.get("/api/workspace-slug-check/", params={"slug": space["slug"]})
    assert res.status_code == 200
    assert res.json() == {"status": False}


def test_slug_check_restricted(user_api):
    res = user_api.get("/api/workspace-slug-check/", params={"slug": "api"})
    assert res.status_code == 200
    assert res.json() == {"status": False}


def test_slug_check_missing(user_api):
    res = user_api.get("/api/workspace-slug-check/")
    assert res.status_code == 400
    assert res.json() == {"error": "Workspace Slug is required"}


def test_slug_check_anon(anon_api):
    # Authenticated-only: anonymous callers get 401, not the status shape.
    res = anon_api.get("/api/workspace-slug-check/", params={"slug": "free-slug"})
    assert res.status_code == 401


# --- workspaces create ---


def test_create_workspace(user_api, world):
    res = user_api.post("/api/workspaces/", json={"name": "Acme", "slug": "acme"})
    assert res.status_code == 201
    body = res.json()
    assert set(body.keys()) == WS_CREATE_KEYS
    assert body["name"] == "Acme"
    assert body["slug"] == "acme"
    assert body["owner"] == world["user"]["id"]
    assert body["total_members"] == 1
    assert body["role"] == 20
    # The creator becomes the workspace admin member.
    with world["db"].connect() as conn:
        row = conn.execute(
            "select role, is_active from workspace_members where workspace_id = %s and member_id = %s;",
            (body["id"], world["user"]["id"]),
        ).fetchone()
    assert row == (20, True)


def test_create_workspace_missing_fields(user_api):
    res = user_api.post("/api/workspaces/", json={})
    assert res.status_code == 400
    assert res.json() == {"error": "Both name and slug are required"}


def test_create_workspace_duplicate_slug(user_api, space):
    res = user_api.post("/api/workspaces/", json={"name": "Acme 2", "slug": space["slug"]})
    assert res.status_code == 400
    assert res.json() == {"slug": ["Workspace with this slug already exists."]}


def test_create_workspace_bad_slug(user_api):
    res = user_api.post("/api/workspaces/", json={"name": "X", "slug": "bad slug!"})
    assert res.status_code == 400
    assert "slug" in res.json()


def test_create_workspace_url_name(user_api):
    res = user_api.post(
        "/api/workspaces/",
        json={"name": "see https://evil.example/x", "slug": "acme2"},
    )
    assert res.status_code == 400
    assert res.json() == {"error": "Name cannot contain a URL"}


def test_create_workspace_anon(anon_api):
    res = anon_api.post("/api/workspaces/", json={"name": "Acme", "slug": "acme"})
    assert res.status_code == 401


# --- workspaces list (frozen bug) ---


def test_list_workspaces_known_bug(user_api, space):
    # BUG (Django, ported): the list action carries
    # @allow_permission(..., level="WORKSPACE"), whose decorator reads
    # kwargs["slug"] — but the collection route has no slug kwarg, so every
    # authenticated list raises KeyError and the base handler maps it to this
    # 400. The Rust port must reproduce the 400 until the bug is fixed.
    res = user_api.get("/api/workspaces/")
    assert res.status_code == 400
    assert res.json() == {"error": "The required key does not exist."}


def test_list_workspaces_anon(anon_api):
    res = anon_api.get("/api/workspaces/")
    assert res.status_code == 401


# --- workspaces retrieve / update / destroy ---


def test_retrieve_workspace(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == WS_KEYS
    assert body["id"] == space["id"]
    assert body["name"] == "Acme"
    assert body["total_members"] == 1


def test_retrieve_workspace_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/")
    assert res.status_code == 401


def test_retrieve_workspace_non_member(other_api, space):
    # Tenant isolation: a workspace you are not a member of reads as missing.
    res = other_api.get(f"/api/workspaces/{space['slug']}/")
    assert res.status_code == 404


def test_retrieve_workspace_missing(user_api):
    res = user_api.get("/api/workspaces/nope/")
    assert res.status_code == 404


def test_patch_workspace(user_api, space):
    res = user_api.patch(f"/api/workspaces/{space['slug']}/", json={"name": "Acme Renamed"})
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == WS_KEYS
    assert body["name"] == "Acme Renamed"


def test_patch_workspace_denied(other_api, space):
    # Denied permission: a signed-in non-member cannot update.
    res = other_api.patch(f"/api/workspaces/{space['slug']}/", json={"name": "Hacked"})
    assert res.status_code == 403


def test_patch_workspace_member_forbidden(member_api, space):
    # Members (role 15) are not workspace admins: update is admin-only.
    res = member_api.patch(f"/api/workspaces/{space['slug']}/", json={"name": "Hacked"})
    assert res.status_code == 403
    assert res.json() == {"error": "You don't have the required permissions."}


def test_put_workspace(user_api, space):
    res = user_api.put(
        f"/api/workspaces/{space['slug']}/", json={"name": "Acme Full", "slug": space["slug"]}
    )
    assert res.status_code == 200
    assert res.json()["name"] == "Acme Full"


def test_delete_workspace(user_api, space):
    res = user_api.delete(f"/api/workspaces/{space['slug']}/")
    assert res.status_code == 204
    assert user_api.get(f"/api/workspaces/{space['slug']}/").status_code == 404


def test_delete_workspace_non_admin(other_api, space):
    res = other_api.delete(f"/api/workspaces/{space['slug']}/")
    assert res.status_code == 403


def test_delete_workspace_member_forbidden(member_api, space):
    res = member_api.delete(f"/api/workspaces/{space['slug']}/")
    assert res.status_code == 403


def test_delete_workspace_missing_slug(user_api):
    # The permission check runs before object lookup: unknown slugs 403.
    res = user_api.delete("/api/workspaces/nope/")
    assert res.status_code == 403


def test_delete_workspace_anon(anon_api, space):
    res = anon_api.delete(f"/api/workspaces/{space['slug']}/")
    assert res.status_code == 401


# --- members ---


def test_list_members(user_api, world, space):
    world["db"].make_workspace_member(space["id"], world["other"]["id"], role=15)
    res = user_api.get(f"/api/workspaces/{space['slug']}/members/")
    assert res.status_code == 200
    body = res.json()
    assert len(body) == 2
    assert set(body[0].keys()) == MEMBER_KEYS
    assert set(body[0]["member"].keys()) == MEMBER_USER_KEYS
    assert {m["role"] for m in body} == {20, 15}


def test_list_members_non_member(other_api, space):
    res = other_api.get(f"/api/workspaces/{space['slug']}/members/")
    assert res.status_code == 403


def test_list_members_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/members/")
    assert res.status_code == 401


def test_retrieve_member(user_api, world, space):
    mid = world["db"].make_workspace_member(space["id"], world["other"]["id"], role=15)["id"]
    res = user_api.get(f"/api/workspaces/{space['slug']}/members/{mid}/")
    assert res.status_code == 200
    body = res.json()
    assert body["id"] == mid
    assert body["role"] == 15
    assert body["member"]["email"] == "other@example.com"


def test_retrieve_member_missing(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/members/{uuid.uuid4()}/")
    assert res.status_code == 404
    assert res.json() == {"error": "Workspace member not found"}


def test_patch_member_role(user_api, world, space):
    mid = world["db"].make_workspace_member(space["id"], world["other"]["id"], role=15)["id"]
    res = user_api.patch(f"/api/workspaces/{space['slug']}/members/{mid}/", json={"role": 5})
    assert res.status_code == 200
    assert res.json()["role"] == 5
    with world["db"].connect() as conn:
        role = conn.execute(
            "select role from workspace_members where id = %s;", (mid,)
        ).fetchone()[0]
    assert role == 5


def test_patch_member_self(user_api, world, space):
    with world["db"].connect() as conn:
        mid = conn.execute(
            "select id from workspace_members where workspace_id = %s and member_id = %s;",
            (space["id"], world["user"]["id"]),
        ).fetchone()[0]
    res = user_api.patch(f"/api/workspaces/{space['slug']}/members/{mid}/", json={"role": 5})
    assert res.status_code == 400
    assert res.json() == {"error": "You cannot update your own role"}


def test_patch_member_non_admin(other_api, world, space):
    world["db"].make_workspace_member(space["id"], world["other"]["id"], role=15)
    with world["db"].connect() as conn:
        mid = conn.execute(
            "select id from workspace_members where workspace_id = %s and member_id = %s;",
            (space["id"], world["other"]["id"]),
        ).fetchone()[0]
    res = other_api.patch(f"/api/workspaces/{space['slug']}/members/{mid}/", json={"role": 5})
    assert res.status_code == 403


def test_delete_member(user_api, world, space):
    mid = world["db"].make_workspace_member(space["id"], world["other"]["id"], role=15)["id"]
    res = user_api.delete(f"/api/workspaces/{space['slug']}/members/{mid}/")
    assert res.status_code == 204
    with world["db"].connect() as conn:
        active = conn.execute(
            "select is_active from workspace_members where id = %s;", (mid,)
        ).fetchone()[0]
    assert active is False


def test_delete_member_self(user_api, world, space):
    with world["db"].connect() as conn:
        mid = conn.execute(
            "select id from workspace_members where workspace_id = %s and member_id = %s;",
            (space["id"], world["user"]["id"]),
        ).fetchone()[0]
    res = user_api.delete(f"/api/workspaces/{space['slug']}/members/{mid}/")
    assert res.status_code == 400
    assert "leave workspace" in res.json()["error"]


def test_leave_only_admin(user_api, space):
    res = user_api.post(f"/api/workspaces/{space['slug']}/members/leave/")
    assert res.status_code == 400
    assert "only admin" in res.json()["error"]


def test_leave_workspace(user_api, other_api, world, space):
    world["db"].make_workspace_member(space["id"], world["other"]["id"], role=20)
    res = user_api.post(f"/api/workspaces/{space['slug']}/members/leave/")
    assert res.status_code == 204
    with world["db"].connect() as conn:
        active = conn.execute(
            "select is_active from workspace_members where workspace_id = %s and member_id = %s;",
            (space["id"], world["user"]["id"]),
        ).fetchone()[0]
    assert active is False
    # The other admin still sees the workspace.
    assert other_api.get(f"/api/workspaces/{space['slug']}/").status_code == 200


# --- workspace-members/me, views, project-members, last-visited ---


def test_members_me(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/workspace-members/me/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == MEMBERS_ME_KEYS
    assert body["role"] == 20
    assert body["draft_issue_count"] == 0


def test_members_me_non_member(other_api, space):
    # Frozen quirk: a non-member gets 200 with an empty (all-null) row.
    res = other_api.get(f"/api/workspaces/{space['slug']}/workspace-members/me/")
    assert res.status_code == 200
    body = res.json()
    assert body["role"] is None
    assert body["member"] is None


def test_members_me_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/workspace-members/me/")
    assert res.status_code == 401


def test_workspace_views(user_api, world, space):
    res = user_api.post(
        f"/api/workspaces/{space['slug']}/workspace-views/", json={"view_props": {"layout": "kanban"}}
    )
    assert res.status_code == 204
    with world["db"].connect() as conn:
        props = conn.execute(
            "select view_props from workspace_members where workspace_id = %s and member_id = %s;",
            (space["id"], world["user"]["id"]),
        ).fetchone()[0]
    assert props["layout"] == "kanban"


def test_workspace_views_anon(anon_api, space):
    res = anon_api.post(f"/api/workspaces/{space['slug']}/workspace-views/", json={"view_props": {}})
    assert res.status_code == 401


def test_project_members_empty(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/project-members/")
    assert res.status_code == 200
    assert res.json() == {}


def test_project_members_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/project-members/")
    assert res.status_code == 401


def test_last_visited_known_bug(user_api):
    # BUG (Django, ported): the view reads user.last_workspace_id, but the
    # User model has no such attribute (it lives on Profile), so every call
    # raises AttributeError and the base handler maps it to this 500. The
    # Rust port must reproduce the 500 until the bug is fixed.
    res = user_api.get("/api/users/last-visited-workspace/")
    assert res.status_code == 500
    assert res.json() == {"error": "Something went wrong please try again later"}


# --- invitations ---


def test_create_invitation(user_api, world, space):
    res = user_api.post(
        f"/api/workspaces/{space['slug']}/invitations/",
        json={"emails": [{"email": "newbie@example.com", "role": 15}]},
    )
    assert res.status_code == 200
    assert res.json() == {"message": "Emails sent successfully"}
    with world["db"].connect() as conn:
        row = conn.execute(
            "select role, accepted from workspace_member_invites where workspace_id = %s and email = %s;",
            (space["id"], "newbie@example.com"),
        ).fetchone()
    assert row == (15, False)


def test_create_invitation_empty(user_api, space):
    res = user_api.post(f"/api/workspaces/{space['slug']}/invitations/", json={})
    assert res.status_code == 400
    assert res.json() == {"error": "Emails are required"}


def test_create_invitation_invalid_email(user_api, space):
    res = user_api.post(
        f"/api/workspaces/{space['slug']}/invitations/",
        json={"emails": [{"email": "nope", "role": 5}]},
    )
    assert res.status_code == 400
    assert "Invalid email" in res.json()["error"]


def test_create_invitation_existing_member(user_api, world, space):
    world["db"].make_workspace_member(space["id"], world["other"]["id"], role=15)
    res = user_api.post(
        f"/api/workspaces/{space['slug']}/invitations/",
        json={"emails": [{"email": "other@example.com", "role": 5}]},
    )
    assert res.status_code == 400
    assert res.json()["error"] == "Some users are already member of workspace"
    assert res.json()["workspace_users"][0]["member"]["display_name"] == "Other User"


def test_create_invitation_higher_role(member_api, space):
    # A role-15 member cannot invite a role-20 admin.
    res = member_api.post(
        f"/api/workspaces/{space['slug']}/invitations/",
        json={"emails": [{"email": "boss@example.com", "role": 20}]},
    )
    assert res.status_code == 400
    assert res.json() == {"error": "You cannot invite a user with higher role"}


def test_create_invitation_by_member(member_api, space):
    # The admin permission admits members too: a role-15 invite succeeds.
    res = member_api.post(
        f"/api/workspaces/{space['slug']}/invitations/",
        json={"emails": [{"email": "guest@example.com", "role": 5}]},
    )
    assert res.status_code == 200


def test_list_invitations(user_api, world, space):
    world["db"].make_invite(space["id"], "s@example.com", created_by_id=world["user"]["id"])
    res = user_api.get(f"/api/workspaces/{space['slug']}/invitations/")
    assert res.status_code == 200
    body = res.json()
    assert len(body) == 1
    assert set(body[0].keys()) == INVITE_KEYS
    assert body[0]["email"] == "s@example.com"
    assert body[0]["workspace"]["slug"] == space["slug"]
    assert body[0]["invite_link"].startswith("/workspace-invitations/?invitation_id=")


def test_list_invitations_non_member(other_api, space):
    res = other_api.get(f"/api/workspaces/{space['slug']}/invitations/")
    assert res.status_code == 403


def test_list_invitations_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/invitations/")
    assert res.status_code == 401


def test_retrieve_invitation(user_api, world, space):
    inv = world["db"].make_invite(space["id"], "s@example.com", created_by_id=world["user"]["id"])
    res = user_api.get(f"/api/workspaces/{space['slug']}/invitations/{inv['id']}/")
    assert res.status_code == 200
    assert set(res.json().keys()) == INVITE_KEYS


def test_retrieve_invitation_non_member(other_api, world, space):
    inv = world["db"].make_invite(space["id"], "s@example.com")
    res = other_api.get(f"/api/workspaces/{space['slug']}/invitations/{inv['id']}/")
    assert res.status_code == 403


def test_patch_invitation(user_api, world, space):
    inv = world["db"].make_invite(space["id"], "s@example.com", created_by_id=world["user"]["id"])
    res = user_api.patch(f"/api/workspaces/{space['slug']}/invitations/{inv['id']}/", json={"role": 5})
    assert res.status_code == 200
    assert res.json()["role"] == 5


def test_patch_invitation_missing(user_api, space):
    res = user_api.patch(
        f"/api/workspaces/{space['slug']}/invitations/{uuid.uuid4()}/", json={"role": 5}
    )
    assert res.status_code == 404


def test_delete_invitation(user_api, world, space):
    inv = world["db"].make_invite(space["id"], "s@example.com", created_by_id=world["user"]["id"])
    res = user_api.delete(f"/api/workspaces/{space['slug']}/invitations/{inv['id']}/")
    assert res.status_code == 204
    # Soft delete: the row is kept with deleted_at set, and drops from reads.
    with world["db"].connect() as conn:
        deleted_at = conn.execute(
            "select deleted_at from workspace_member_invites where id = %s;", (inv["id"],)
        ).fetchone()[0]
    assert deleted_at is not None
    assert user_api.get(f"/api/workspaces/{space['slug']}/invitations/").json() == []


# --- invitation join ---


def test_join_detail_shape(anon_api, world, space):
    # The join detail is AllowAny: even anonymous callers can read it.
    inv = world["db"].make_invite(space["id"], "newbie@example.com", token="tok-abc")
    res = anon_api.get(f"/api/workspaces/{space['slug']}/invitations/{inv['id']}/join/")
    assert res.status_code == 200
    assert set(res.json().keys()) == INVITE_KEYS
    assert res.json()["token"] == "tok-abc"


def test_join_wrong_token(other_api, world, space):
    inv = world["db"].make_invite(
        space["id"], "other@example.com", token="tok-real", created_by_id=world["user"]["id"]
    )
    res = other_api.post(
        f"/api/workspaces/{space['slug']}/invitations/{inv['id']}/join/",
        json={"token": "tok-wrong", "accepted": True},
    )
    assert res.status_code == 403
    assert res.json() == {"error": "You do not have permission to join the workspace"}


def test_join_reject_then_double_respond(other_api, world, space):
    inv = world["db"].make_invite(
        space["id"], "other@example.com", token="tok-r", created_by_id=world["user"]["id"]
    )
    res = other_api.post(
        f"/api/workspaces/{space['slug']}/invitations/{inv['id']}/join/",
        json={"token": "tok-r", "accepted": False},
    )
    assert res.status_code == 200
    assert res.json() == {"message": "Workspace Invitation was not accepted"}
    res = other_api.post(
        f"/api/workspaces/{space['slug']}/invitations/{inv['id']}/join/",
        json={"token": "tok-r", "accepted": True},
    )
    assert res.status_code == 400
    assert res.json() == {"error": "You have already responded to the invitation request"}


def test_join_accept(other_api, world, space):
    inv = world["db"].make_invite(
        space["id"], "other@example.com", token="tok-ok", created_by_id=world["user"]["id"]
    )
    res = other_api.post(
        f"/api/workspaces/{space['slug']}/invitations/{inv['id']}/join/",
        json={"token": "tok-ok", "accepted": True},
    )
    assert res.status_code == 200
    assert res.json() == {"message": "Workspace Invitation Accepted"}
    with world["db"].connect() as conn:
        member = conn.execute(
            "select role, is_active from workspace_members where workspace_id = %s and member_id = %s;",
            (space["id"], world["other"]["id"]),
        ).fetchone()
        deleted_at = conn.execute(
            "select deleted_at from workspace_member_invites where id = %s;", (inv["id"],)
        ).fetchone()[0]
    assert member == (15, True)
    assert deleted_at is not None


def test_join_accept_unknown_email(user_api, world, space):
    # The invite targets an email with no account: the accept message is
    # returned but no membership is created and the invite is kept.
    inv = world["db"].make_invite(
        space["id"], "ghost@example.com", token="tok-g", created_by_id=world["user"]["id"]
    )
    res = user_api.post(
        f"/api/workspaces/{space['slug']}/invitations/{inv['id']}/join/",
        json={"token": "tok-g", "accepted": True},
    )
    assert res.status_code == 200
    assert res.json() == {"message": "Workspace Invitation Accepted"}
    with world["db"].connect() as conn:
        count = conn.execute(
            "select count(*) from workspace_member_invites where id = %s;", (inv["id"],)
        ).fetchone()[0]
    assert count == 1


# --- users/me invitations ---


def test_my_invitations(other_api, world, space):
    world["db"].make_invite(
        space["id"], "other@example.com", token="tok-mine", created_by_id=world["user"]["id"]
    )
    res = other_api.get("/api/users/me/workspaces/invitations/")
    assert res.status_code == 200
    body = res.json()
    assert len(body) == 1
    assert set(body[0].keys()) == INVITE_KEYS


def test_my_invitations_empty(user_api):
    assert user_api.get("/api/users/me/workspaces/invitations/").json() == []


def test_my_invitations_anon(anon_api):
    assert anon_api.get("/api/users/me/workspaces/invitations/").status_code == 401


def test_accept_invitation_via_me(other_api, world, space):
    inv = world["db"].make_invite(
        space["id"], "other@example.com", token="tok-me", created_by_id=world["user"]["id"]
    )
    res = other_api.post("/api/users/me/workspaces/invitations/", json={"invitations": [inv["id"]]})
    assert res.status_code == 204
    with world["db"].connect() as conn:
        member = conn.execute(
            "select role, is_active from workspace_members where workspace_id = %s and member_id = %s;",
            (space["id"], world["other"]["id"]),
        ).fetchone()
        deleted_at = conn.execute(
            "select deleted_at from workspace_member_invites where id = %s;", (inv["id"],)
        ).fetchone()[0]
    assert member == (15, True)
    assert deleted_at is not None


# --- join requests ---


def test_join_request_create(other_api, world, space):
    res = other_api.post(
        "/api/users/me/workspaces/join-requests/",
        json={"admin_email": "user@example.com", "message": "let me in"},
    )
    assert res.status_code == 201
    assert res.json() == {"message": "Request sent"}
    with world["db"].connect() as conn:
        row = conn.execute(
            "select status, message from workspace_join_requests where workspace_id = %s and requester_id = %s;",
            (space["id"], world["other"]["id"]),
        ).fetchone()
    assert row == ("PENDING", "let me in")


def test_join_request_bad_email(other_api):
    res = other_api.post(
        "/api/users/me/workspaces/join-requests/", json={"admin_email": "not-an-email"}
    )
    assert res.status_code == 400
    assert res.json() == {"error": "A valid workspace admin email is required"}


def test_join_request_own_email(user_api):
    res = user_api.post(
        "/api/users/me/workspaces/join-requests/", json={"admin_email": "user@example.com"}
    )
    assert res.status_code == 400
    assert res.json() == {"error": "You cannot request to join a workspace using your own email"}


def test_join_request_unresolved_neutral(other_api, world):
    # An unknown admin email returns the same neutral message and still
    # records an unresolved (workspace-less) request.
    res = other_api.post(
        "/api/users/me/workspaces/join-requests/", json={"admin_email": "ghost@example.com"}
    )
    assert res.status_code == 201
    assert res.json() == {"message": "Request sent"}
    with world["db"].connect() as conn:
        row = conn.execute(
            "select workspace_id, status from workspace_join_requests where requester_id = %s;",
            (world["other"]["id"],),
        ).fetchone()
    assert row == (None, "PENDING")


def test_join_request_already_member(member_api, space):
    # other@example.com is already a role-15 member of acme, whose admin is
    # user@example.com: routed straight back with the slug.
    res = member_api.post(
        "/api/users/me/workspaces/join-requests/", json={"admin_email": "user@example.com"}
    )
    assert res.status_code == 200
    assert res.json() == {
        "message": "You are already a member of this workspace",
        "workspace_slug": space["slug"],
    }


def test_join_request_anon(anon_api):
    res = anon_api.post(
        "/api/users/me/workspaces/join-requests/", json={"admin_email": "user@example.com"}
    )
    assert res.status_code == 401


def test_list_my_join_requests(other_api, world, space):
    world["db"].make_join_request(
        world["other"]["id"], "user@example.com", workspace_id=space["id"], message="hi"
    )
    res = other_api.get("/api/users/me/workspaces/join-requests/")
    assert res.status_code == 200
    body = res.json()
    assert len(body) == 1
    assert set(body[0].keys()) == MY_JOIN_REQUEST_KEYS
    assert body[0]["status"] == "PENDING"
    assert body[0]["requester"]["display_name"] == "Other User"


def test_list_join_requests_admin(user_api, world, space):
    world["db"].make_join_request(world["other"]["id"], "user@example.com", workspace_id=space["id"])
    res = user_api.get(f"/api/workspaces/{space['slug']}/join-requests/")
    assert res.status_code == 200
    body = res.json()
    assert len(body) == 1
    assert set(body[0].keys()) == ADMIN_JOIN_REQUEST_KEYS
    assert body[0]["workspace"]["slug"] == space["slug"]


def test_list_join_requests_non_admin(other_api, world, space):
    world["db"].make_join_request(world["other"]["id"], "user@example.com", workspace_id=space["id"])
    res = other_api.get(f"/api/workspaces/{space['slug']}/join-requests/")
    assert res.status_code == 403


def test_list_join_requests_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/join-requests/")
    assert res.status_code == 401


def test_approve_join_request(user_api, other_api, world, space):
    world["db"].make_join_request(world["other"]["id"], "user@example.com", workspace_id=space["id"])
    with world["db"].connect() as conn:
        jid = conn.execute(
            "select id from workspace_join_requests where workspace_id = %s;", (space["id"],)
        ).fetchone()[0]
    res = user_api.post(f"/api/workspaces/{space['slug']}/join-requests/{jid}/approve/")
    assert res.status_code == 200
    assert res.json() == {"message": "Request approved"}
    with world["db"].connect() as conn:
        member = conn.execute(
            "select role, is_active from workspace_members where workspace_id = %s and member_id = %s;",
            (space["id"], world["other"]["id"]),
        ).fetchone()
        status = conn.execute(
            "select status from workspace_join_requests where id = %s;", (jid,)
        ).fetchone()[0]
        last_ws = conn.execute(
            "select last_workspace_id from profiles where user_id = %s;", (world["other"]["id"],)
        ).fetchone()[0]
    assert member == (15, True)
    assert status == "APPROVED"
    assert str(last_ws) == space["id"]
    # The new member now sees the workspace.
    assert other_api.get(f"/api/workspaces/{space['slug']}/").status_code == 200


def test_approve_join_request_twice(user_api, world, space):
    jid = world["db"].make_join_request(
        world["other"]["id"], "user@example.com", workspace_id=space["id"]
    )["id"]
    assert user_api.post(f"/api/workspaces/{space['slug']}/join-requests/{jid}/approve/").status_code == 200
    res = user_api.post(f"/api/workspaces/{space['slug']}/join-requests/{jid}/approve/")
    assert res.status_code == 400
    assert res.json() == {"error": "This request has already been responded to"}


def test_deny_join_request(user_api, world, space):
    jid = world["db"].make_join_request(
        world["other"]["id"], "user@example.com", workspace_id=space["id"]
    )["id"]
    res = user_api.post(f"/api/workspaces/{space['slug']}/join-requests/{jid}/deny/")
    assert res.status_code == 200
    assert res.json() == {"message": "Request denied"}
    with world["db"].connect() as conn:
        row = conn.execute(
            "select status from workspace_join_requests where id = %s;", (jid,)
        ).fetchone()[0]
        members = conn.execute(
            "select count(*) from workspace_members where workspace_id = %s and member_id = %s;",
            (space["id"], world["other"]["id"]),
        ).fetchone()[0]
    assert row == "DENIED"
    assert members == 0


def test_deny_join_request_non_admin(member_api, world, space):
    jid = world["db"].make_join_request(
        world["user"]["id"], "user@example.com", workspace_id=space["id"]
    )["id"]
    res = member_api.post(f"/api/workspaces/{space['slug']}/join-requests/{jid}/deny/")
    assert res.status_code == 403

