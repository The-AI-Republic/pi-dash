"""User assets v2: avatar/cover presigned uploads scoped to the caller."""

from . import seed_assets as seed_a


def base():
    return "/api/assets/v2/user-assets/"


def detail(asset_id):
    return f"/api/assets/v2/user-assets/{asset_id}/"


def payload(**kw):
    body = {
        "name": "avatar.png",
        "type": "image/png",
        "size": 512,
        "entity_type": "USER_AVATAR",
    }
    body.update(kw)
    return body


def test_post_shape(org):
    r = org.request("POST", base(), "admin", json=payload())
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body.keys()) == {"upload_data", "asset_id", "asset_url"}
    assert set(body["upload_data"].keys()) == {"url", "fields"}
    assert body["upload_data"]["fields"]["Content-Type"] == "image/png"
    assert body["upload_data"]["fields"]["key"].endswith("-avatar.png")
    key = body["upload_data"]["fields"]["key"]
    assert "/" not in key  # bare "<hex>-<name>", no workspace prefix
    assert body["asset_url"] == f"/api/assets/v2/static/{body['asset_id']}/"
    row = seed_a.fetch(org.conn, body["asset_id"])
    assert row["user_id"] == org.admin["id"]
    assert row["workspace_id"] is None
    assert row["entity_type"] == "USER_AVATAR"


def test_post_user_cover_shape(org):
    r = org.request("POST", base(), "admin", json=payload(entity_type="USER_COVER"))
    assert r.status_code == 200, r.text
    assert r.json()["asset_url"].startswith("/api/assets/v2/static/")


def test_post_invalid_entity_type(org):
    r = org.request("POST", base(), "admin", json=payload(entity_type="WORKSPACE_LOGO"))
    assert r.status_code == 400
    assert r.json() == {"error": "Invalid entity type.", "status": False}


def test_post_invalid_file_type(org):
    r = org.request(
        "POST", base(), "admin",
        json=payload(name="doc.pdf", type="application/pdf"),
    )
    assert r.status_code == 400
    assert r.json() == {
        "error": "Invalid file type. Only JPEG, PNG, WebP, JPG and GIF files are allowed.",
        "status": False,
    }


def test_patch_confirms_and_links_avatar(org):
    asset_id = org.request("POST", base(), "admin", json=payload()).json()["asset_id"]
    r = org.request(
        "PATCH", detail(asset_id), "admin",
        json={"attributes": {"name": "avatar.png", "picked": True}},
    )
    assert r.status_code == 204, r.text
    row = seed_a.fetch(org.conn, asset_id)
    assert row["is_uploaded"] is True
    assert row["attributes"] == {"name": "avatar.png", "picked": True}
    link = org.conn.execute(
        "SELECT avatar_asset_id FROM users WHERE id = %s", (org.admin["id"],)
    ).fetchone()[0]
    assert str(link) == asset_id


def test_patch_other_users_asset_404(org):
    asset_id = org.request("POST", base(), "admin", json=payload()).json()["asset_id"]
    r = org.request("PATCH", detail(asset_id), "member", json={})
    assert r.status_code == 404
    assert r.json() == {"error": "The required object does not exist."}


def test_delete_and_cross_user_404(org):
    asset_id = org.request("POST", base(), "admin", json=payload()).json()["asset_id"]
    r = org.request("DELETE", detail(asset_id), "member")
    assert r.status_code == 404
    assert r.json() == {"error": "The required object does not exist."}
    r = org.request("DELETE", detail(asset_id), "admin")
    assert r.status_code == 204
    row = seed_a.fetch(org.conn, asset_id)
    assert row["is_deleted"] is True
    assert row["deleted_at"] is not None
    link = org.conn.execute(
        "SELECT avatar_asset_id FROM users WHERE id = %s", (org.admin["id"],)
    ).fetchone()[0]
    assert link is None
