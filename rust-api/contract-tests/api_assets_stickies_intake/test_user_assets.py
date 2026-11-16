"""User-asset endpoints: presigned upload, mark-uploaded, delete (+ server variants).

``UserAssetEndpoint`` (``POST /api/v1/assets/user-assets/``,
``PATCH``/``DELETE /api/v1/assets/user-assets/<uuid>/``) and
``UserServerAssetEndpoint`` (same shapes under ``.../server/``). These views
carry no workspace/project permission class — any authenticated caller may
mint uploads, but patch/delete resolve the row by ``(id, user_id)`` so one
user's asset is invisible to another (404, ``FileAsset.DoesNotExist``).
"""

USER = "/api/v1/assets/user-assets/"
SERVER = "/api/v1/assets/user-assets/server/"

UPLOAD = {"name": "profile.jpg", "type": "image/jpeg", "size": 1024000, "entity_type": "USER_AVATAR"}


def _upload(org, role="admin", base=USER, **overrides):
    payload = {**UPLOAD, **overrides}
    r = org.request("POST", base, role, json=payload)
    assert r.status_code == 200, r.text[:500]
    return r.json()


def test_user_asset_upload_shape(org):
    body = _upload(org)
    assert set(body) == {"upload_data", "asset_id", "asset_url"}
    assert set(body["upload_data"]) == {"url", "fields"}
    assert body["upload_data"]["url"].startswith("http")
    assert body["upload_data"]["fields"]["key"].endswith("-profile.jpg")
    # USER_AVATAR assets resolve to the static-asset route.
    assert body["asset_url"] == f"/api/assets/v2/static/{body['asset_id']}/"


def test_user_asset_upload_invalid_entity_type(org):
    r = org.request("POST", USER, "admin", json={**UPLOAD, "entity_type": "ISSUE_ATTACHMENT"})
    assert r.status_code == 400
    assert r.json() == {"error": "Invalid entity type.", "status": False}


def test_user_asset_upload_missing_entity_type(org):
    payload = {k: v for k, v in UPLOAD.items() if k != "entity_type"}
    r = org.request("POST", USER, "admin", json=payload)
    assert r.status_code == 400
    assert r.json() == {"error": "Invalid entity type.", "status": False}


def test_user_asset_upload_invalid_file_type(org):
    r = org.request("POST", USER, "admin", json={**UPLOAD, "type": "application/pdf"})
    assert r.status_code == 400
    assert r.json() == {
        "error": "Invalid file type. Only JPEG and PNG files are allowed.",
        "status": False,
    }


def test_user_asset_patch_marks_uploaded(org):
    asset_id = _upload(org)["asset_id"]
    r = org.request("PATCH", f"{USER}{asset_id}/", "admin", json={"attributes": {"caption": "me"}})
    assert r.status_code == 204
    assert r.text == ""
    row = org.conn.execute(
        "SELECT is_uploaded, attributes FROM file_assets WHERE id = %s", (asset_id,)
    ).fetchone()
    assert row[0] is True
    assert row[1] == {"caption": "me"}


def test_user_asset_patch_unknown_id_404(org):
    r = org.request(
        "PATCH", f"{USER}123e4567-e89b-12d3-a456-426614174000/", "admin", json={"attributes": {}}
    )
    assert r.status_code == 404


def test_user_asset_patch_other_users_asset_404(org):
    asset_id = _upload(org, role="admin")["asset_id"]
    r = org.request("PATCH", f"{USER}{asset_id}/", "member", json={"attributes": {}})
    assert r.status_code == 404


def test_user_asset_delete(org):
    asset_id = _upload(org)["asset_id"]
    r = org.request("DELETE", f"{USER}{asset_id}/", "admin")
    assert r.status_code == 204
    assert r.text == ""
    row = org.conn.execute(
        "SELECT is_deleted, deleted_at FROM file_assets WHERE id = %s", (asset_id,)
    ).fetchone()
    assert row[0] is True
    assert row[1] is not None
    # The first DELETE stamps deleted_at, which hides the row from the default
    # manager, so a second DELETE resolves nothing.
    r = org.request("DELETE", f"{USER}{asset_id}/", "admin")
    assert r.status_code == 404


def test_user_asset_anonymous_401(org):
    r = org.request("POST", USER, None, json=UPLOAD)
    assert r.status_code == 401
    assert r.json() == {"detail": "Authentication credentials were not provided."}


def test_server_asset_upload_shape(org):
    body = _upload(org, base=SERVER)
    assert set(body) == {"upload_data", "asset_id", "asset_url"}
    assert set(body["upload_data"]) == {"url", "fields"}
    assert body["upload_data"]["url"].startswith("https://")
    assert body["upload_data"]["fields"]["key"].endswith("-profile.jpg")
    assert body["asset_url"] == f"/api/assets/v2/static/{body['asset_id']}/"


def _server_detail(asset_id):
    return f"/api/v1/assets/user-assets/{asset_id}/server/"


def test_server_asset_patch_and_delete(org):
    asset_id = _upload(org, base=SERVER)["asset_id"]
    r = org.request("PATCH", _server_detail(asset_id), "admin", json={"attributes": {}})
    assert r.status_code == 204
    r = org.request("DELETE", _server_detail(asset_id), "admin")
    assert r.status_code == 204
    row = org.conn.execute(
        "SELECT is_deleted FROM file_assets WHERE id = %s", (asset_id,)
    ).fetchone()
    assert row[0] is True


def test_server_asset_upload_invalid_entity_type(org):
    r = org.request("POST", SERVER, "admin", json={**UPLOAD, "entity_type": "NOPE"})
    assert r.status_code == 400
    assert r.json() == {"error": "Invalid entity type.", "status": False}
