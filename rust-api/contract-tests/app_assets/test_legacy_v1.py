"""Legacy v1 file assets: workspace rows, user rows, and restore.

The v1 surface is ``FileAssetSerializer`` over ``file_assets`` rows keyed by
the ``asset`` column. Reads and deletes need no object storage; multipart
uploads always fail before the network (``S3Storage`` never initialised its
django-storages settings) and user reads always fail on an unserializable
queryset — both ported bugs are pinned exactly.
"""

from . import seed_assets as seed_a

V1_ROW_KEYS = {
    "id", "created_at", "updated_at", "deleted_at", "attributes", "asset",
    "entity_type", "entity_identifier", "is_deleted", "is_archived",
    "external_id", "external_source", "size", "is_uploaded",
    "storage_metadata", "created_by", "updated_by", "user", "workspace",
    "draft_issue", "project", "issue", "comment", "page",
}


def ws_row_path(org, leaf):
    return f"/api/workspaces/file-assets/{org.workspace['id']}/{leaf}/"


def seed_ws_row(org, leaf, **kw):
    return seed_a.create_asset(
        org.conn,
        workspace_id=org.workspace["id"],
        created_by_id=org.admin["id"],
        name=leaf,
        entity_type="ISSUE_ATTACHMENT",
        uploaded=True,
        **kw,
    )


def test_workspace_get_shape(org):
    row = seed_ws_row(org, "shape.txt")
    key = row["key"]
    leaf = key.split("/", 1)[1]
    r = org.request("GET", ws_row_path(org, leaf), "admin")
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body.keys()) == {"data", "status"}
    assert body["status"] is True
    assert len(body["data"]) == 1
    item = body["data"][0]
    assert set(item.keys()) == V1_ROW_KEYS
    assert item["id"] == row["id"]
    assert item["attributes"] == {"name": "shape.txt", "type": "image/png", "size": 100}
    assert key in item["asset"]
    assert item["created_at"].endswith("Z")
    assert item["entity_type"] == "ISSUE_ATTACHMENT"
    assert item["workspace"] == org.workspace["id"]
    assert item["created_by"] == org.admin["id"]


def test_workspace_get_missing_key_200(org):
    r = org.request("GET", ws_row_path(org, "no-such-file.txt"), "admin")
    assert r.status_code == 200
    assert r.json() == {"error": "Asset key does not exist", "status": False}


def test_workspace_post_file_500_storage_bug(org):
    # Ported bug: S3Storage.__init__ never calls super().__init__, so
    # django-storages' file_overwrite is missing and every real upload 500s.
    r = org.client("admin").post(
        f"/api/workspaces/{org.workspace['slug']}/file-assets/",
        files={"asset": ("f.txt", b"hello", "text/plain")},
    )
    assert r.status_code == 500
    assert r.json() == {"error": "Something went wrong please try again later"}


def test_workspace_post_without_file_400(org):
    r = org.request(
        "POST", f"/api/workspaces/{org.workspace['slug']}/file-assets/",
        "admin", data={},
    )
    assert r.status_code == 400
    assert r.json() == {"asset": ["No file was submitted."]}


def test_workspace_post_file_unknown_workspace_404(org):
    r = org.client("admin").post(
        "/api/workspaces/no-such-ws/file-assets/",
        files={"asset": ("f.txt", b"hello", "text/plain")},
    )
    assert r.status_code == 404
    assert r.json() == {"error": "The required object does not exist."}


def test_workspace_delete_leaves_row_visible(org):
    # Ported quirk: v1 delete sets only is_deleted (no deleted_at), and the
    # default manager filters on deleted_at — so the row still reads back.
    row = seed_ws_row(org, "gone.txt")
    leaf = row["key"].split("/", 1)[1]
    r = org.request("DELETE", ws_row_path(org, leaf), "admin")
    assert r.status_code == 204
    assert r.text == ""
    assert seed_a.fetch(org.conn, row["id"])["is_deleted"] is True
    r = org.request("GET", ws_row_path(org, leaf), "admin")
    assert r.status_code == 200
    assert r.json()["status"] is True
    assert r.json()["data"][0]["id"] == row["id"]
    # … and the v2 existence check (deleted_at based) still reports true.
    r = org.request(
        "GET",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}/check/{row['id']}/",
        "admin",
    )
    assert r.json() == {"exists": True}


def test_workspace_delete_unknown_key_404(org):
    r = org.request("DELETE", ws_row_path(org, "no-such-file.txt"), "admin")
    assert r.status_code == 404
    assert r.json() == {"error": "The required object does not exist."}


def test_workspace_restore_round_trip(org):
    row = seed_ws_row(org, "back.txt")
    leaf = row["key"].split("/", 1)[1]
    org.request("DELETE", ws_row_path(org, leaf), "admin")
    r = org.request("POST", ws_row_path(org, leaf) + "restore/", "admin")
    assert r.status_code == 204
    assert r.text == ""
    assert seed_a.fetch(org.conn, row["id"])["is_deleted"] is False
    r = org.request("GET", ws_row_path(org, leaf), "admin")
    assert r.json()["status"] is True


def test_workspace_restore_unknown_key_404(org):
    r = org.request("POST", ws_row_path(org, "no-such-file.txt") + "restore/", "admin")
    assert r.status_code == 404
    assert r.json() == {"error": "The required object does not exist."}


def test_user_get_existing_500_serializer_bug(org):
    # Ported bug: UserAssetsEndpoint.get serializes a queryset without
    # many=True, so any existing user asset 500s.
    row = seed_a.create_asset(
        org.conn,
        workspace_id=None,
        created_by_id=org.admin["id"],
        user_id=org.admin["id"],
        name="mine.txt",
        uploaded=True,
    )
    leaf = row["key"]
    r = org.request("GET", f"/api/users/file-assets/{leaf}/", "admin")
    assert r.status_code == 500
    assert r.json() == {"error": "Something went wrong please try again later"}


def test_user_get_missing_key_200(org):
    r = org.request("GET", "/api/users/file-assets/no-such-key/", "admin")
    assert r.status_code == 200
    assert r.json() == {"error": "Asset key does not exist", "status": False}


def test_user_post_file_500_storage_bug(org):
    r = org.client("admin").post(
        "/api/users/file-assets/",
        files={"asset": ("u.txt", b"hi", "text/plain")},
    )
    assert r.status_code == 500
    assert r.json() == {"error": "Something went wrong please try again later"}


def test_user_post_without_file_400(org):
    r = org.request("POST", "/api/users/file-assets/", "admin", data={})
    assert r.status_code == 400
    assert r.json() == {"asset": ["No file was submitted."]}


def test_user_post_json_415(org):
    r = org.request("POST", "/api/users/file-assets/", "admin", json={"x": 1})
    assert r.status_code == 415
    assert "Unsupported media type" in r.json()["detail"]


def test_user_delete_scoped_to_creator(org):
    row = seed_a.create_asset(
        org.conn,
        workspace_id=None,
        created_by_id=org.admin["id"],
        user_id=org.admin["id"],
        name="bye.txt",
        uploaded=True,
    )
    leaf = row["key"]
    r = org.request("DELETE", f"/api/users/file-assets/{leaf}/", "member")
    assert r.status_code == 404
    assert r.json() == {"error": "The required object does not exist."}
    r = org.request("DELETE", f"/api/users/file-assets/{leaf}/", "admin")
    assert r.status_code == 204
    assert seed_a.fetch(org.conn, row["id"])["is_deleted"] is True
