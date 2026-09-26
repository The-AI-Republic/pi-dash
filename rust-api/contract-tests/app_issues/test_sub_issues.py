"""Sub-issue shapes (``issues/<id>/sub-issues/``).

Seed: I2 is the child of I1 (backlog). The assign test uses an
API-created throwaway child and deletes it, leaving the seed intact.
"""

SUB_KEYS = {
    "id", "name", "state_id", "sort_order", "completed_at",
    "estimate_point", "priority", "start_date", "target_date",
    "sequence_id", "project_id", "parent_id", "cycle_id", "module_ids",
    "label_ids", "assignee_ids", "sub_issues_count", "created_at",
    "updated_at", "created_by", "updated_by", "attachment_count",
    "link_count", "is_draft", "archived_at", "state_group",
}


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def test_sub_issues_get_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, i1, i2 = (
        seed["ws_slug"], seed["project"], seed["issue1"], seed["issue2"])
    resp = admin.get(f"{_base(ws, pid)}/issues/{i1}/sub-issues/")
    assert resp.status_code == 200
    body = resp.json()
    assert set(body) == {"sub_issues", "state_distribution"}
    assert isinstance(body["sub_issues"], list)
    assert len(body["sub_issues"]) == 1
    row = body["sub_issues"][0]
    assert set(row) == SUB_KEYS
    assert row["id"] == i2
    assert row["parent_id"] == i1
    assert row["state_group"] == "backlog"
    assert row["sub_issues_count"] == 0
    assert row["attachment_count"] == 0
    assert body["state_distribution"] == {"backlog": [i2]}


def test_sub_issues_get_grouped(clients, seed):
    admin = clients["admin"]
    ws, pid, i1, i2 = (
        seed["ws_slug"], seed["project"], seed["issue1"], seed["issue2"])
    resp = admin.get(
        f"{_base(ws, pid)}/issues/{i1}/sub-issues/?group_by=state_group")
    assert resp.status_code == 200
    body = resp.json()
    assert set(body["sub_issues"]) == {"backlog"}
    assert [row["id"] for row in body["sub_issues"]["backlog"]] == [i2]
    assert body["state_distribution"] == {"backlog": [i2]}


def test_sub_issues_assign_and_cleanup(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    created = admin.post(
        f"{_base(ws, pid)}/issues/", json={"name": "Future child"}).json()
    child_id = created["id"]
    try:
        resp = admin.post(
            f"{_base(ws, pid)}/issues/{i1}/sub-issues/",
            json={"sub_issue_ids": [child_id]},
        )
        assert resp.status_code == 200
        body = resp.json()
        assert set(body) == {"sub_issues", "state_distribution"}
        assert body["sub_issues"][0]["parent_id"] == i1
        assert body["state_distribution"] == {"backlog": [child_id]}
        listing = admin.get(
            f"{_base(ws, pid)}/issues/{i1}/sub-issues/").json()
        assert child_id in {row["id"] for row in listing["sub_issues"]}
    finally:
        assert admin.delete(
            f"{_base(ws, pid)}/issues/{child_id}/").status_code == 204
    listing = admin.get(f"{_base(ws, pid)}/issues/{i1}/sub-issues/").json()
    # Only the seeded child remains; the throwaway was deleted.
    assert {row["id"] for row in listing["sub_issues"]} == {seed["issue2"]}


def test_sub_issues_assign_empty_is_400(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.post(
        f"{_base(ws, pid)}/issues/{i1}/sub-issues/",
        json={"sub_issue_ids": []},
    )
    assert resp.status_code == 400
    assert resp.json() == {"error": "Sub Issue IDs are required"}
