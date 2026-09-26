"""Workspace file assets v2: presigned upload mint, confirm, fetch, delete.

``POST /api/assets/v2/workspaces/<slug>/`` mints a presigned S3 POST
(offline signing — no object storage needed); ``PATCH`` confirms the upload
and links the entity; ``GET`` redirects to a presigned download URL once
``is_uploaded`` is set; ``DELETE`` soft-deletes with ``deleted_at``.
"""

import base64
import json

from . import seed_assets as seed_a

UPLOAD_KEYS = {"upload_data", "asset_id", "asset_url"}


def ws_base(org):
    return f"/api/assets/v2/workspaces/{org.workspace['slug']}/"


def ws_detail(org, asset_id):
    return f"/api/assets/v2/workspaces/{org.workspace['slug']}/{asset_id}/"


def ws_payload(org, name="logo.png", **kw):
    payload = {
        "name": name,
        "type": "image/png",
        "size": 1024,
        "entity_type": "WORKSPACE_LOGO",
        "entity_identifier": org.workspace["id"],
    }
    payload.update(kw)
    return payload


def policy_conditions(upload_data):
    raw = upload_data["fields"]["policy"]
    return json.loads(base64.b64decode(raw + "=="))["conditions"]


def length_range_max(conditions):
    for cond in conditions:
        if isinstance(cond, list) and cond[:2] == ["content-length-range", 1]:
            return cond[2]
    raise AssertionError(f"no content-length-range in {conditions}")


def test_post_shape(org):
    r = org.request("POST", ws_base(org), "admin", json=ws_payload(org))
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body.keys()) == UPLOAD_KEYS
    assert set(body["upload_data"].keys()) == {"url", "fields"}
    fields = body["upload_data"]["fields"]
    assert fields["Content-Type"] == "image/png"
    assert fields["key"].endswith("-logo.png")
    assert fields["key"].startswith(f"{org.workspace['id']}/")
    assert f"/{org.workspace['slug']}/" not in fields["key"]
    assert body["asset_url"] == f"/api/assets/v2/static/{body['asset_id']}/"
    conds = policy_conditions(body["upload_data"])
    assert length_range_max(conds) == 1024
    row = seed_a.fetch(org.conn, body["asset_id"])
    assert row["workspace_id"] == org.workspace["id"]
    assert row["entity_type"] == "WORKSPACE_LOGO"
    assert row["size"] == 1024
    assert row["attributes"] == {"name": "logo.png", "type": "image/png", "size": 1024}
    assert row["is_uploaded"] is False


def test_post_defaults(org):
    # Omitted type defaults to image/jpeg; omitted size to FILE_SIZE_LIMIT.
    r = org.request(
        "POST", ws_base(org), "admin",
        json={"name": "d.png", "entity_type": "WORKSPACE_LOGO",
              "entity_identifier": org.workspace["id"]},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["upload_data"]["fields"]["Content-Type"] == "image/jpeg"
    row = seed_a.fetch(org.conn, body["asset_id"])
    conds = policy_conditions(body["upload_data"])
    assert length_range_max(conds) == row["size"]
    assert row["attributes"]["type"] == "image/jpeg"


def test_post_size_clamped_to_limit(org):
    r = org.request("POST", ws_base(org), "admin", json=ws_payload(org, size=10**9))
    assert r.status_code == 200, r.text
    body = r.json()
    row = seed_a.fetch(org.conn, body["asset_id"])
    assert row["size"] < 10**9
    assert row["size"] == row["attributes"]["size"]
    assert length_range_max(policy_conditions(body["upload_data"])) == row["size"]


def test_post_invalid_entity_type(org):
    r = org.request(
        "POST", ws_base(org), "admin", json=ws_payload(org, entity_type="NOPE")
    )
    assert r.status_code == 400
    assert r.json() == {"error": "Invalid entity type.", "status": False}


def test_post_invalid_file_type(org):
    r = org.request(
        "POST", ws_base(org), "admin",
        json=ws_payload(org, name="doc.pdf", type="application/pdf"),
    )
    assert r.status_code == 400
    assert r.json() == {
        "error": "Invalid file type. Only JPEG, PNG, WebP, JPG and GIF files are allowed.",
        "status": False,
    }


def test_get_before_upload_404(org):
    r = org.request("POST", ws_base(org), "admin", json=ws_payload(org))
    asset_id = r.json()["asset_id"]
    r = org.request("GET", ws_detail(org, asset_id), "admin")
    assert r.status_code == 404
    assert r.json() == {"error": "The requested asset could not be found."}


def test_patch_confirms_upload_and_attrs(org):
    asset_id = org.request("POST", ws_base(org), "admin", json=ws_payload(org)).json()["asset_id"]
    r = org.request(
        "PATCH", ws_detail(org, asset_id), "admin",
        json={"attributes": {"name": "logo.png", "custom": 7}},
    )
    assert r.status_code == 204, r.text
    assert r.text == ""
    row = seed_a.fetch(org.conn, asset_id)
    assert row["is_uploaded"] is True
    assert row["attributes"] == {"name": "logo.png", "custom": 7}
    # The workspace logo link follows the confirmed asset.
    link = org.conn.execute(
        "SELECT logo_asset_id FROM workspaces WHERE id = %s",
        (org.workspace["id"],),
    ).fetchone()[0]
    assert str(link) == asset_id


def test_get_after_upload_redirects(org):
    asset_id = org.request("POST", ws_base(org), "admin", json=ws_payload(org)).json()["asset_id"]
    seed_a.mark_uploaded(org.conn, asset_id)
    r = org.request("GET", ws_detail(org, asset_id), "admin")
    assert r.status_code == 302, r.text
    location = r.headers["location"]
    row = seed_a.fetch(org.conn, asset_id)
    assert row["asset"] in location
    assert "response-content-disposition=attachment" in location
    assert "logo.png" in location


def test_get_unknown_asset_404(org):
    r = org.request(
        "GET", ws_detail(org, "00000000-0000-0000-0000-000000000000"), "admin"
    )
    assert r.status_code == 404
    assert r.json() == {"error": "The required object does not exist."}


def test_get_wrong_workspace_slug_404(org):
    asset_id = org.request("POST", ws_base(org), "admin", json=ws_payload(org)).json()["asset_id"]
    seed_a.mark_uploaded(org.conn, asset_id)
    r = org.request(
        "GET", f"/api/assets/v2/workspaces/no-such-ws-{org.tag}/{asset_id}/", "admin"
    )
    assert r.status_code == 404
    assert r.json() == {"error": "The required object does not exist."}


def test_patch_unknown_asset_404(org):
    r = org.request(
        "PATCH", ws_detail(org, "00000000-0000-0000-0000-000000000000"),
        "admin", json={},
    )
    assert r.status_code == 404
    assert r.json() == {"error": "The required object does not exist."}


def test_delete_then_get_and_check(org):
    asset_id = org.request("POST", ws_base(org), "admin", json=ws_payload(org)).json()["asset_id"]
    seed_a.mark_uploaded(org.conn, asset_id)
    r = org.request("DELETE", ws_detail(org, asset_id), "admin")
    assert r.status_code == 204
    assert r.text == ""
    row = seed_a.fetch(org.conn, asset_id)
    assert row["is_deleted"] is True
    assert row["deleted_at"] is not None
    # The soft-deleted row leaves the default manager: detail 404s …
    r = org.request("GET", ws_detail(org, asset_id), "admin")
    assert r.status_code == 404
    assert r.json() == {"error": "The required object does not exist."}
    # … and the existence check reports false.
    r = org.request(
        "GET",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}/check/{asset_id}/",
        "admin",
    )
    assert r.status_code == 200
    assert r.json() == {"exists": False}
