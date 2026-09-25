"""Version + move shapes.

Seed: one issue-version row and one description-version row on I1. The
move test relocates J1 from IS2 to IS and back, so the seed is restored.
"""

VERSION_PAGE_KEYS = {
    "prev_cursor", "cursor", "next_cursor", "prev_page_results",
    "next_page_results", "page_count", "total_results", "total_pages",
    "results",
}

VERSION_ROW_KEYS = {
    "id", "workspace", "project", "issue", "last_saved_at", "owned_by",
    "created_at", "updated_at", "created_by", "updated_by",
}

VERSION_DETAIL_KEYS = {
    "id", "workspace", "project", "issue", "parent", "state",
    "estimate_point", "name", "priority", "start_date", "target_date",
    "assignees", "sequence_id", "labels", "sort_order", "completed_at",
    "archived_at", "is_draft", "external_source", "external_id",
    "type", "cycle", "modules", "meta", "last_saved_at", "owned_by",
    "created_at", "updated_at", "created_by", "updated_by",
}

DESC_DETAIL_KEYS = {
    "id", "workspace", "project", "issue", "description_binary",
    "description_html", "description_stripped", "description_json",
    "last_saved_at", "owned_by", "created_at", "updated_at",
    "created_by", "updated_by",
}

FORBIDDEN = {"error": "You don't have the required permissions."}


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def test_issue_versions_list_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.get(f"{_base(ws, pid)}/issues/{i1}/versions/")
    assert resp.status_code == 200
    body = resp.json()
    assert set(body) == VERSION_PAGE_KEYS
    assert body["total_results"] == 1
    assert set(body["results"][0]) == VERSION_ROW_KEYS
    assert body["results"][0]["id"] == seed["version1"]
    assert body["results"][0]["issue"] == i1
    assert body["results"][0]["owned_by"] == seed["admin"]


def test_issue_version_detail_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.get(
        f"{_base(ws, pid)}/issues/{i1}/versions/{seed['version1']}/")
    assert resp.status_code == 200
    body = resp.json()
    assert set(body) == VERSION_DETAIL_KEYS
    assert body["name"] == "I1 parent"
    assert body["priority"] == "high"
    assert body["sequence_id"] == 1
    assert body["assignees"] == []
    assert body["labels"] == []
    assert body["modules"] == []
    assert body["meta"] == {}


def test_description_versions_shapes(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.get(
        f"{_base(ws, pid)}/work-items/{i1}/description-versions/")
    assert resp.status_code == 200
    body = resp.json()
    assert set(body) == VERSION_PAGE_KEYS
    assert body["total_results"] == 1
    assert set(body["results"][0]) == VERSION_ROW_KEYS
    assert body["results"][0]["id"] == seed["desc_version1"]
    resp = admin.get(
        f"{_base(ws, pid)}/work-items/{i1}/description-versions/"
        f"{seed['desc_version1']}/")
    assert resp.status_code == 200
    detail = resp.json()
    assert set(detail) == DESC_DETAIL_KEYS
    assert detail["description_html"] == "<p>seed</p>"
    assert detail["description_stripped"] == "seed"


def test_move_roundtrip(clients, seed):
    # J1 lives on IS2 (which has a default backlog state, as does IS);
    # move it to IS and back so the seed is restored.
    admin = clients["admin"]
    ws, p2, pid = seed["ws_slug"], seed["project2"], seed["project"]
    j1 = seed["j1"]
    resp = admin.post(
        f"{_base(ws, p2)}/work-items/{j1}/move/", json={"project": "IS"})
    assert resp.status_code == 200
    assert resp.json()["project_id"] == pid
    assert admin.get(f"{_base(ws, pid)}/issues/{j1}/").status_code == 200
    # Move back so the seed is restored for the other files.
    resp = admin.post(
        f"{_base(ws, pid)}/work-items/{j1}/move/", json={"project": "IS2"})
    assert resp.status_code == 200
    assert resp.json()["project_id"] == p2
    assert admin.get(f"{_base(ws, p2)}/issues/{j1}/").status_code == 200


def test_move_requires_target(clients, seed):
    admin = clients["admin"]
    ws, p2, j1 = seed["ws_slug"], seed["project2"], seed["j1"]
    resp = admin.post(f"{_base(ws, p2)}/work-items/{j1}/move/", json={})
    assert resp.status_code == 400
    assert resp.json() == {"error": "project is required"}


def test_move_guest_denied(clients, seed):
    guest = clients["guest"]
    ws, p2, j1 = seed["ws_slug"], seed["project2"], seed["j1"]
    resp = guest.post(
        f"{_base(ws, p2)}/work-items/{j1}/move/", json={"project": "IS"})
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN


def test_move_rejects_non_member_target(clients, seed):
    # Admin is not a member of the other tenant's project ISX, so the
    # target does not even resolve (tenant isolation by 404).
    admin = clients["admin"]
    ws, p2, j1 = seed["ws_slug"], seed["project2"], seed["j1"]
    resp = admin.post(
        f"{_base(ws, p2)}/work-items/{j1}/move/", json={"project": "ISX"})
    assert resp.status_code == 404
    assert resp.json() == {"detail": "Project not found"}
