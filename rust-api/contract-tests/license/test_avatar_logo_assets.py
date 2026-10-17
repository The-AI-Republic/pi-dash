# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Avatar/logo URL parity for soft-deleted and odd-entity assets (PIDASHCONV-1115).

Django resolves ``user.avatar_asset`` / ``workspace.logo_asset`` through the
forward-FK descriptor, which uses the unscoped ``_base_manager``, so
soft-deleted assets still render via ``FileAsset.asset_url``. Unknown entity
types yield ``None`` (no fallback to the raw string), and a NULL-workspace
attachment/description asset raises ``AttributeError`` inside the property —
which DRF's ``Field.get_attribute`` converts to ``SkipField``, so the key is
*omitted* with 200/201, not a 500.
"""

import uuid

from _harness import admin_client
from _harness.settings import secret_key

FALLBACK = "https://cdn/fallback.png"


def make_asset(db, entity_type, *, deleted=False, workspace_id=None):
    aid = str(uuid.uuid4())
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                """INSERT INTO file_assets
                   (id, attributes, asset, entity_type, workspace_id,
                    is_deleted, is_archived, size, is_uploaded,
                    created_at, updated_at, deleted_at)
                   VALUES (%s, '{}', 'ct/avatar.png', %s, %s,
                    false, false, 0, false, now(), now(),
                    CASE WHEN %s THEN now() ELSE NULL END)""",
                (aid, entity_type, workspace_id, deleted),
            )
        conn.commit()
    return aid


def set_avatar(db, user_id, asset_id, avatar=FALLBACK):
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "UPDATE users SET avatar_asset_id = %s, avatar = %s WHERE id = %s;",
                (asset_id, avatar, user_id),
            )
        conn.commit()


def set_logo(db, workspace_id, asset_id, logo=FALLBACK):
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "UPDATE workspaces SET logo_asset_id = %s, logo = %s WHERE id = %s;",
                (asset_id, logo, workspace_id),
            )
        conn.commit()


def make_case_admin(db, instance_id, email, asset_id):
    user = db.make_user(email)
    set_avatar(db, user["id"], asset_id)
    db.make_admin(instance_id, user["id"])
    return user


def admin_session_for(db, user):
    return db.mint_admin_session(user, secret_key())


# --- avatar: me (fetch_user site) -------------------------------------------


def test_avatar_soft_deleted_asset_renders_entity_url(world):
    asset = make_asset(world["db"], "USER_AVATAR", deleted=True)
    user = make_case_admin(world["db"], world["instance"]["id"], "ava@example.com", asset)
    with admin_client(admin_session_for(world["db"], user)) as client:
        res = client.get("/api/instances/admins/me/")
    assert res.status_code == 200
    assert res.json()["avatar_url"] == f"/api/assets/v2/static/{asset}/"


def test_avatar_null_entity_renders_null(world):
    asset = make_asset(world["db"], None)
    user = make_case_admin(world["db"], world["instance"]["id"], "avb@example.com", asset)
    with admin_client(admin_session_for(world["db"], user)) as client:
        res = client.get("/api/instances/admins/me/")
    assert res.status_code == 200
    body = res.json()
    assert "avatar_url" in body
    assert body["avatar_url"] is None


def test_avatar_null_workspace_attachment_omits_key(world):
    asset = make_asset(world["db"], "ISSUE_ATTACHMENT")
    user = make_case_admin(world["db"], world["instance"]["id"], "avc@example.com", asset)
    with admin_client(admin_session_for(world["db"], user)) as client:
        res = client.get("/api/instances/admins/me/")
    assert res.status_code == 200
    assert "avatar_url" not in res.json()


def test_avatar_attachment_with_workspace_renders_url(world):
    ws = world["db"].make_workspace("AvAttach", "avattach", world["admin"]["id"])
    asset = make_asset(world["db"], "ISSUE_ATTACHMENT", workspace_id=ws["id"])
    user = make_case_admin(world["db"], world["instance"]["id"], "avd@example.com", asset)
    with admin_client(admin_session_for(world["db"], user)) as client:
        res = client.get("/api/instances/admins/me/")
    assert res.status_code == 200
    assert res.json()["avatar_url"] == (
        f"/api/assets/v2/workspaces/avattach/projects/None/issues/None/attachments/{asset}/"
    )


def test_avatar_fallback_branches(world):
    live = make_asset(world["db"], "USER_AVATAR")
    plain = make_case_admin(world["db"], world["instance"]["id"], "plain@example.com", live)
    set_avatar(world["db"], plain["id"], None, avatar="https://cdn/x.png")
    bare = make_case_admin(world["db"], world["instance"]["id"], "bare@example.com", live)
    set_avatar(world["db"], bare["id"], None, avatar="")
    with admin_client(admin_session_for(world["db"], plain)) as client:
        body = client.get("/api/instances/admins/me/").json()
    assert body["avatar_url"] == "https://cdn/x.png"
    with admin_client(admin_session_for(world["db"], bare)) as client:
        body = client.get("/api/instances/admins/me/").json()
    assert body["avatar_url"] is None


# --- avatar: list (separate join site) --------------------------------------


def test_admin_list_renders_avatar_branches(admin_api, world):
    db = world["db"]
    soft = make_asset(db, "USER_AVATAR", deleted=True)
    nullent = make_asset(db, None)
    attach = make_asset(db, "ISSUE_ATTACHMENT")
    make_case_admin(db, world["instance"]["id"], "la@example.com", soft)
    make_case_admin(db, world["instance"]["id"], "lb@example.com", nullent)
    make_case_admin(db, world["instance"]["id"], "lc@example.com", attach)
    res = admin_api.get("/api/instances/admins/")
    assert res.status_code == 200
    by_email = {e["user_detail"]["email"]: e["user_detail"] for e in res.json()}
    assert by_email["la@example.com"]["avatar_url"] == f"/api/assets/v2/static/{soft}/"
    assert "avatar_url" in by_email["lb@example.com"]
    assert by_email["lb@example.com"]["avatar_url"] is None
    assert "avatar_url" not in by_email["lc@example.com"]


# --- logo: list -------------------------------------------------------------


def test_logo_soft_deleted_asset_renders_entity_url(admin_api, world):
    db = world["db"]
    asset = make_asset(db, "WORKSPACE_LOGO", deleted=True)
    ws = db.make_workspace("LogoA", "logo-a", world["admin"]["id"])
    set_logo(db, ws["id"], asset)
    res = admin_api.get("/api/instances/workspaces/")
    assert res.status_code == 200
    (entry,) = [w for w in res.json()["results"] if w["slug"] == "logo-a"]
    assert entry["logo_url"] == f"/api/assets/v2/static/{asset}/"


def test_logo_null_entity_renders_null(admin_api, world):
    db = world["db"]
    asset = make_asset(db, None)
    ws = db.make_workspace("LogoB", "logo-b", world["admin"]["id"])
    set_logo(db, ws["id"], asset)
    res = admin_api.get("/api/instances/workspaces/")
    assert res.status_code == 200
    (entry,) = [w for w in res.json()["results"] if w["slug"] == "logo-b"]
    assert "logo_url" in entry
    assert entry["logo_url"] is None


def test_logo_null_workspace_attachment_omits_key(admin_api, world):
    db = world["db"]
    asset = make_asset(db, "ISSUE_ATTACHMENT")
    ws = db.make_workspace("LogoC", "logo-c", world["admin"]["id"])
    set_logo(db, ws["id"], asset)
    res = admin_api.get("/api/instances/workspaces/")
    assert res.status_code == 200
    (entry,) = [w for w in res.json()["results"] if w["slug"] == "logo-c"]
    assert "logo_url" not in entry


def test_logo_attachment_with_workspace_renders_url(admin_api, world):
    db = world["db"]
    ws = db.make_workspace("LogoD", "logo-d", world["admin"]["id"])
    asset = make_asset(db, "ISSUE_ATTACHMENT", workspace_id=ws["id"])
    set_logo(db, ws["id"], asset)
    res = admin_api.get("/api/instances/workspaces/")
    assert res.status_code == 200
    (entry,) = [w for w in res.json()["results"] if w["slug"] == "logo-d"]
    assert entry["logo_url"] == (
        f"/api/assets/v2/workspaces/logo-d/projects/None/issues/None/attachments/{asset}/"
    )


# --- logo: create -----------------------------------------------------------


def test_create_logo_null_entity_renders_null(admin_api, world):
    asset = make_asset(world["db"], None)
    res = admin_api.post(
        "/api/instances/workspaces/",
        json={"name": "CB", "slug": "create-b", "logo": FALLBACK, "logo_asset": asset},
    )
    assert res.status_code == 201
    assert "logo_url" in res.json()
    assert res.json()["logo_url"] is None


def test_create_logo_null_workspace_attachment_omits_key(admin_api, world):
    asset = make_asset(world["db"], "ISSUE_ATTACHMENT")
    res = admin_api.post(
        "/api/instances/workspaces/",
        json={"name": "CC", "slug": "create-c", "logo": FALLBACK, "logo_asset": asset},
    )
    assert res.status_code == 201
    assert "logo_url" not in res.json()


def test_create_logo_soft_deleted_asset_rejected(admin_api, world):
    asset = make_asset(world["db"], "WORKSPACE_LOGO", deleted=True)
    res = admin_api.post(
        "/api/instances/workspaces/",
        json={"name": "CA", "slug": "create-a", "logo": FALLBACK, "logo_asset": asset},
    )
    assert res.status_code == 400
    assert res.json() == {"logo_asset": [f'Invalid pk "{asset}" - object does not exist.']}
