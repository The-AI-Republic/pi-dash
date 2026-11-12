"""Attachment shapes (v1 multipart + v2 presigned flows).

Storage limits (the suite runs without S3/MinIO):

- v1 upload and v1 delete touch the broken ``S3Storage`` subclass
  (``__init__`` never calls ``super().__init__()``, so
  ``file_overwrite``/``location`` are missing) and always 500 — pinned
  as a ported bug. The 400/404 branches plus list shapes are covered.
- v2 valid-type POST needs a bucket to presign against, so only the
  invalid-type 400 is asserted; list/detail-pending/patch/delete are
  covered. The uploaded-asset redirect branch needs S3 and is not
  asserted here.

Test order: the pending-asset 400 runs before the PATCH that flips it
to uploaded; the list test asserting both seeded rows runs before the
v2 delete at the bottom.
"""

ASSET_KEYS = {
    "id", "asset_url", "created_at", "updated_at", "deleted_at",
    "attributes", "asset", "entity_type", "entity_identifier",
    "is_deleted", "is_archived", "external_id", "external_source",
    "size", "is_uploaded", "storage_metadata", "created_by",
    "updated_by", "user", "workspace", "draft_issue", "project",
    "issue", "comment", "page",
}


def _v1(ws, pid, iid):
    return f"/api/workspaces/{ws}/projects/{pid}/issues/{iid}/issue-attachments"


def _v2(ws, pid, iid):
    return (f"/api/assets/v2/workspaces/{ws}/projects/{pid}/issues/"
            f"{iid}/attachments")


def test_v1_list_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.get(_v1(ws, pid, i1) + "/")
    assert resp.status_code == 200
    rows = resp.json()
    assert {row["id"] for row in rows} == {seed["asset1"], seed["asset2"]}
    for row in rows:
        assert set(row) == ASSET_KEYS
        assert row["entity_type"] == "ISSUE_ATTACHMENT"
    by_id = {row["id"]: row for row in rows}
    assert by_id[seed["asset1"]]["is_uploaded"] is True
    assert by_id[seed["asset1"]]["attributes"] == {"name": "seed.txt"}
    assert by_id[seed["asset2"]]["is_uploaded"] is False


def test_v1_post_empty_is_400(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.post(_v1(ws, pid, i1) + "/")
    assert resp.status_code == 400
    assert resp.json() == {"asset": ["No file was submitted."]}


def test_v1_post_valid_file_500s(clients, seed):
    # Ported bug: ``S3Storage.__init__`` never calls
    # ``super().__init__()``, so ``file_overwrite`` is missing and every
    # v1 upload 500s, with or without object storage configured.
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.post(
        _v1(ws, pid, i1) + "/",
        files={"asset": ("probe.txt", b"hello probe", "text/plain")},
    )
    assert resp.status_code == 500
    assert resp.json() == {"error": "Something went wrong please try again later"}


def test_v1_delete_missing_is_404(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.delete(
        _v1(ws, pid, i1) + "/00000000-0000-0000-0000-000000000000/")
    assert resp.status_code == 404
    assert resp.json() == {"error": "Issue attachment not found."}


def test_v1_delete_without_file_500s(clients, seed):
    # Same broken storage: deleting a row whose file was never stored
    # 500s in ``storage.delete`` (missing ``location``). The row itself
    # survives — the 500 fires before ``issue_attachment.delete()``.
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.delete(_v1(ws, pid, i1) + f"/{seed['asset1']}/")
    assert resp.status_code == 500
    assert resp.json() == {"error": "Something went wrong please try again later"}
    assert seed["asset1"] in {
        row["id"] for row in admin.get(_v1(ws, pid, i1) + "/").json()
    }


def test_v2_list_only_uploaded(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.get(_v2(ws, pid, i1) + "/")
    assert resp.status_code == 200
    rows = resp.json()
    assert [row["id"] for row in rows] == [seed["asset1"]]
    assert set(rows[0]) == ASSET_KEYS
    assert rows[0]["asset_url"].endswith(
        f"/attachments/{seed['asset1']}/")


def test_v2_post_invalid_type_is_400(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.post(
        _v2(ws, pid, i1) + "/",
        json={"name": "x.exe", "type": "application/x-msdownload",
              "size": 10},
    )
    assert resp.status_code == 400
    assert resp.json() == {"error": "Invalid file type.", "status": False}


def test_v2_detail_pending_is_400(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.get(_v2(ws, pid, i1) + f"/{seed['asset2']}/")
    assert resp.status_code == 400
    assert resp.json() == {"error": "The asset is not uploaded.",
                           "status": False}


def test_v2_patch_marks_uploaded(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.patch(_v2(ws, pid, i1) + f"/{seed['asset2']}/")
    assert resp.status_code == 204
    rows = admin.get(_v2(ws, pid, i1) + "/").json()
    assert {row["id"] for row in rows} == {seed["asset1"], seed["asset2"]}


def test_v2_delete(clients, seed):
    # Soft-delete only; touches no storage. Runs last: it removes the
    # seeded uploaded asset.
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.delete(_v2(ws, pid, i1) + f"/{seed['asset1']}/")
    assert resp.status_code == 204
