"""Generic workspace assets: upload, download URL, mark-uploaded.

``GenericAssetEndpoint`` (``POST /api/v1/workspaces/<slug>/assets/``,
``GET``/``PATCH /api/v1/workspaces/<slug>/assets/<uuid>/``). Any
authenticated caller may mint an upload; download/patch resolve the row by
``(id, workspace, is_deleted=False)``. The download shape
(``asset_id``/``asset_url``/``asset_name``/``asset_type``) is the wire format
installed runners consume, so it is pinned exactly.
"""

import uuid


def _base(org):
    return f"/api/v1/workspaces/{org.workspace['slug']}/assets/"


def _upload(org, role="admin", **overrides):
    payload = {"name": "report.pdf", "type": "application/pdf", "size": 204800, **overrides}
    r = org.request("POST", _base(org), role, json=payload)
    assert r.status_code == 200, r.text[:500]
    return r.json()


def test_generic_upload_shape(org):
    body = _upload(org)
    assert set(body) == {"upload_data", "asset_id", "asset_url"}
    assert set(body["upload_data"]) == {"url", "fields"}
    assert body["upload_data"]["url"].startswith("http")
    assert body["upload_data"]["fields"]["key"].endswith("-report.pdf")
    # No project/issue bound yet: the attachment URL carries None segments.
    assert body["asset_url"] == (
        f"/api/assets/v2/workspaces/{org.workspace['slug']}/projects/None"
        f"/issues/None/attachments/{body['asset_id']}/"
    )


def test_generic_upload_with_project_and_external_tracking(org):
    body = _upload(
        org,
        project_id=org.project["id"],
        external_id="ext-123",
        external_source="github",
    )
    assert set(body) == {"upload_data", "asset_id", "asset_url"}
    assert f"/projects/{org.project['id']}/" in body["asset_url"]


def test_generic_upload_conflict_on_external_id(org):
    _upload(org, external_id="dup-1", external_source="github")
    payload = {
        "name": "other.pdf",
        "type": "application/pdf",
        "size": 100,
        "external_id": "dup-1",
        "external_source": "github",
    }
    r = org.request("POST", _base(org), "admin", json=payload)
    assert r.status_code == 409
    body = r.json()
    assert set(body) == {"message", "asset_id", "asset_url"}
    assert body["message"] == "Asset with same external id and source already exists"


def test_generic_upload_missing_name_or_size(org):
    r = org.request("POST", _base(org), "admin", json={"type": "application/pdf"})
    assert r.status_code == 400
    assert r.json() == {"error": "Name and size are required fields.", "status": False}


def test_generic_upload_invalid_file_type(org):
    r = org.request(
        "POST", _base(org), "admin",
        json={"name": "x.bin", "type": "application/x-sh", "size": 100},
    )
    assert r.status_code == 400
    assert r.json() == {"error": "Invalid file type.", "status": False}


def test_generic_download_before_upload_400(org):
    asset_id = _upload(org)["asset_id"]
    r = org.request("GET", f"{_base(org)}{asset_id}/", "admin")
    assert r.status_code == 400
    assert r.json() == {"error": "Asset not yet uploaded"}


def test_generic_patch_then_download_shape(org):
    asset_id = _upload(org)["asset_id"]
    r = org.request("PATCH", f"{_base(org)}{asset_id}/", "admin", json={"is_uploaded": True})
    assert r.status_code == 204
    assert r.text == ""
    r = org.request("GET", f"{_base(org)}{asset_id}/", "admin")
    assert r.status_code == 200
    body = r.json()
    assert set(body) == {"asset_id", "asset_url", "asset_name", "asset_type"}
    assert body["asset_id"] == asset_id
    assert body["asset_name"] == "report.pdf"
    assert body["asset_type"] == "application/pdf"
    assert body["asset_url"].startswith("http")
    assert "response-content-disposition" in body["asset_url"]


def test_generic_download_unknown_asset_404(org):
    r = org.request("GET", f"{_base(org)}{uuid.uuid4()}/", "admin")
    assert r.status_code == 404
    assert r.json() == {"error": "Asset not found"}


def test_generic_download_unknown_workspace_404(org):
    asset_id = _upload(org)["asset_id"]
    org.conn.execute(
        "UPDATE file_assets SET is_uploaded = true WHERE id = %s", (asset_id,)
    )
    r = org.request("GET", f"/api/v1/workspaces/nope-{org.tag}/assets/{asset_id}/", "admin")
    assert r.status_code == 404
    assert r.json() == {"error": "Workspace not found"}


def test_generic_download_cross_workspace_404(org):
    asset_id = _upload(org)["asset_id"]
    org.conn.execute(
        "UPDATE file_assets SET is_uploaded = true WHERE id = %s", (asset_id,)
    )
    r = org.request(
        "GET",
        f"/api/v1/workspaces/{org.workspace2['slug']}/assets/{asset_id}/",
        "other_admin",
    )
    assert r.status_code == 404
    assert r.json() == {"error": "Asset not found"}


def test_generic_download_anonymous_401(org):
    asset_id = _upload(org)["asset_id"]
    r = org.request("GET", f"{_base(org)}{asset_id}/", None)
    assert r.status_code == 401
