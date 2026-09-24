"""Archive + bulk-operation shapes.

Seed: I5 is archived (completed state). The archive round-trip test
archives I3 and unarchives it again; bulk tests use API-created
throwaways so the seed is untouched.
"""

ARCHIVE_DETAIL_KEYS = {
    "id", "name", "state_id", "sort_order", "completed_at",
    "estimate_point", "priority", "complexity_score", "start_date",
    "target_date", "sequence_id", "project_id", "parent_id",
    "assigned_pod_id", "agent_executor", "created_at", "updated_at",
    "created_by", "updated_by", "is_draft", "archived_at", "is_synced",
    "description_html", "is_subscribed", "agent_ticker", "agent_status",
    "relations_summary", "has_open_blockers",
}

FORBIDDEN = {"error": "You don't have the required permissions."}


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def test_archived_list_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, i5 = seed["ws_slug"], seed["project"], seed["issue5"]
    resp = admin.get(f"{_base(ws, pid)}/archived-issues/")
    assert resp.status_code == 200
    body = resp.json()
    assert i5 in {row["id"] for row in body["results"]}
    row = next(r for r in body["results"] if r["id"] == i5)
    assert row["name"] == "I5 archived"
    assert row["archived_at"] is not None


def test_archived_list_guest_denied(clients, seed):
    guest = clients["guest"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = guest.get(f"{_base(ws, pid)}/archived-issues/")
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN


def test_archive_retrieve_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, i5 = seed["ws_slug"], seed["project"], seed["issue5"]
    resp = admin.get(f"{_base(ws, pid)}/issues/{i5}/archive/")
    assert resp.status_code == 200
    body = resp.json()
    assert set(body) == ARCHIVE_DETAIL_KEYS
    assert body["name"] == "I5 archived"
    assert body["archived_at"] is not None
    assert body["relations_summary"] == {"blocked_by": [], "blocking": []}


def test_archive_roundtrip_on_completed(clients, seed):
    admin = clients["admin"]
    ws, pid, i3, i5 = (
        seed["ws_slug"], seed["project"], seed["issue3"], seed["issue5"])
    resp = admin.post(f"{_base(ws, pid)}/issues/{i3}/archive/")
    assert resp.status_code == 200
    assert set(resp.json()) == {"archived_at"}
    try:
        archived = {
            row["id"] for row in
            admin.get(f"{_base(ws, pid)}/archived-issues/").json()["results"]
        }
        assert {i3, i5} <= archived
    finally:
        assert admin.request(
            "DELETE", f"{_base(ws, pid)}/issues/{i3}/archive/").status_code == 204
    archived = {
        row["id"] for row in
        admin.get(f"{_base(ws, pid)}/archived-issues/").json()["results"]
    }
    assert i3 not in archived
    assert i5 in archived


def test_archive_open_state_is_400(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.post(f"{_base(ws, pid)}/issues/{i1}/archive/")
    assert resp.status_code == 400
    assert resp.json() == {
        "error": "Can only archive completed or cancelled state group issue"}


def test_bulk_archive_roundtrip(clients, seed):
    admin = clients["admin"]
    ws, pid, done = (
        seed["ws_slug"], seed["project"], seed["state_done"])
    made = [
        admin.post(f"{_base(ws, pid)}/issues/",
                   json={"name": f"Bulk {n}"}).json()
        for n in ("a", "b")
    ]
    try:
        for row in made:
            assert admin.patch(
                f"{_base(ws, pid)}/issues/{row['id']}/",
                json={"state_id": done},
            ).status_code == 204
        resp = admin.post(
            f"{_base(ws, pid)}/bulk-archive-issues/",
            json={"issue_ids": [row["id"] for row in made]},
        )
        assert resp.status_code == 200
        assert set(resp.json()) == {"archived_at"}
        archived = {
            row["id"] for row in admin.get(
                f"{_base(ws, pid)}/archived-issues/").json()["results"]
        }
        assert {row["id"] for row in made} <= archived
    finally:
        for row in made:
            admin.request(
                "DELETE", f"{_base(ws, pid)}/issues/{row['id']}/archive/")
            admin.delete(f"{_base(ws, pid)}/issues/{row['id']}/")


def test_bulk_archive_open_state_is_400(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.post(
        f"{_base(ws, pid)}/bulk-archive-issues/", json={"issue_ids": [i1]})
    assert resp.status_code == 400
    assert resp.json() == {
        "error_code": 4091, "error_message": "INVALID_ARCHIVE_STATE_GROUP"}


def test_bulk_archive_empty_is_400(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.post(
        f"{_base(ws, pid)}/bulk-archive-issues/", json={"issue_ids": []})
    assert resp.status_code == 400
    assert resp.json() == {"error": "Issue IDs are required"}


def test_bulk_delete_roundtrip(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    made = [
        admin.post(f"{_base(ws, pid)}/issues/",
                   json={"name": f"Gone {n}"}).json()
        for n in ("a", "b")
    ]
    resp = admin.request(
        "DELETE", f"{_base(ws, pid)}/bulk-delete-issues/",
        json={"issue_ids": [row["id"] for row in made]},
    )
    assert resp.status_code == 200
    assert resp.json() == {"message": "2 issues were deleted"}
    for row in made:
        assert admin.get(
            f"{_base(ws, pid)}/issues/{row['id']}/").status_code == 404


def test_bulk_delete_empty_is_400(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.request(
        "DELETE", f"{_base(ws, pid)}/bulk-delete-issues/",
        json={"issue_ids": []},
    )
    assert resp.status_code == 400
    assert resp.json() == {"error": "Issue IDs are required"}


def test_bulk_delete_member_denied(clients, seed):
    member = clients["member"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = member.request(
        "DELETE", f"{_base(ws, pid)}/bulk-delete-issues/",
        json={"issue_ids": [i1]},
    )
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN
