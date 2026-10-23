"""Sub-issue shapes (``issues/<id>/sub-issues/``).

Seed: I2 is the child of I1 (backlog). The assign and pagination tests use
API-created throwaway children and delete them, leaving the seed intact.

GET is one ``OffsetPaginator`` page (``per_page`` default 50, max 100):
the ``paginate()`` envelope with ``results`` renamed to ``sub_issues`` and
``state_distribution`` (a rollup over the whole child set) appended.
"""

import pytest

SUB_KEYS = {
    "id", "name", "state_id", "sort_order", "completed_at",
    "estimate_point", "priority", "start_date", "target_date",
    "sequence_id", "project_id", "parent_id", "cycle_id", "module_ids",
    "label_ids", "assignee_ids", "sub_issues_count", "created_at",
    "updated_at", "created_by", "updated_by", "attachment_count",
    "link_count", "is_draft", "archived_at", "state_group",
}

GET_KEYS = [
    "grouped_by", "sub_grouped_by", "total_count", "next_cursor",
    "prev_cursor", "next_page_results", "prev_page_results", "count",
    "total_pages", "total_results", "extra_stats", "sub_issues",
    "state_distribution",
]


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def test_sub_issues_get_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, i1, i2 = (
        seed["ws_slug"], seed["project"], seed["issue1"], seed["issue2"])
    resp = admin.get(f"{_base(ws, pid)}/issues/{i1}/sub-issues/")
    assert resp.status_code == 200
    body = resp.json()
    assert list(body) == GET_KEYS
    assert body["grouped_by"] is None
    assert body["sub_grouped_by"] is None
    assert body["total_count"] == body["total_results"] == 1
    assert body["next_cursor"] == "50:1:0"
    assert body["prev_cursor"] == "50:-1:1"
    assert body["next_page_results"] is False
    assert body["prev_page_results"] is False
    assert body["count"] == 1
    assert body["total_pages"] == 1
    assert body["extra_stats"] is None
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
    assert list(body) == GET_KEYS
    assert body["grouped_by"] is None
    assert body["count"] == 1
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


@pytest.fixture
def wide_parent(clients, seed):
    """A throwaway parent with five API-created children, three of them
    completed; everything is deleted afterwards."""
    admin = clients["admin"]
    base = _base(seed["ws_slug"], seed["project"])
    parent = admin.post(f"{base}/issues/", json={"name": "Wide parent"}).json()
    children = []
    try:
        for index in range(5):
            payload = {"name": f"Wide child {index}", "parent_id": parent["id"]}
            if index % 2 == 0:
                payload["state_id"] = seed["state_done"]
            created = admin.post(f"{base}/issues/", json=payload)
            assert created.status_code == 201
            children.append(created.json()["id"])
        yield {"base": base, "parent": parent["id"], "children": children}
    finally:
        for child in children:
            admin.delete(f"{base}/issues/{child}/")
        admin.delete(f"{base}/issues/{parent['id']}/")


def test_sub_issues_pages_are_bounded_and_walkable(clients, wide_parent):
    admin = clients["admin"]
    url = f"{wide_parent['base']}/issues/{wide_parent['parent']}/sub-issues/"
    first = admin.get(f"{url}?per_page=2")
    assert first.status_code == 200
    page = first.json()
    assert list(page) == GET_KEYS
    assert page["count"] == 2
    assert page["total_count"] == page["total_results"] == 5
    assert page["total_pages"] == 3
    assert page["next_cursor"] == "2:1:0"
    assert page["next_page_results"] is True
    distribution = page["state_distribution"]
    assert set(distribution) == {"backlog", "completed"}
    assert sorted(i for ids in distribution.values() for i in ids) == sorted(
        wide_parent["children"])
    assert len(distribution["completed"]) == 3
    seen = [row["id"] for row in page["sub_issues"]]
    while page["next_page_results"]:
        resp = admin.get(f"{url}?per_page=2&cursor={page['next_cursor']}")
        assert resp.status_code == 200
        page = resp.json()
        assert len(page["sub_issues"]) <= 2
        # The rollup covers the whole set on every page, keys and order.
        assert page["state_distribution"] == distribution
        assert list(page["state_distribution"]) == list(distribution)
        seen.extend(row["id"] for row in page["sub_issues"])
    assert page["prev_page_results"] is True
    assert page["count"] == 1
    assert len(seen) == len(set(seen)) == 5
    # Default order is newest first.
    assert seen == list(reversed(wide_parent["children"]))


def test_sub_issues_default_page_holds_small_parent(clients, wide_parent):
    admin = clients["admin"]
    url = f"{wide_parent['base']}/issues/{wide_parent['parent']}/sub-issues/"
    body = admin.get(url).json()
    assert body["count"] == 5
    assert body["next_page_results"] is False
    assert body["next_cursor"] == "50:1:0"


def test_sub_issues_order_by_pages_in_paginator_order(clients, wide_parent):
    admin = clients["admin"]
    url = f"{wide_parent['base']}/issues/{wide_parent['parent']}/sub-issues/"
    rows = admin.get(f"{url}?order_by=sort_order&per_page=100").json()["sub_issues"]
    orders = [row["sort_order"] for row in rows]
    assert orders == sorted(orders)
    grouped = admin.get(
        f"{url}?order_by=state__group&group_by=state_group&per_page=3").json()
    assert grouped["count"] == 3
    assert sum(len(rows) for rows in grouped["sub_issues"].values()) == 3
    assert set(grouped["state_distribution"]) == {"backlog", "completed"}


def test_sub_issues_per_page_above_max_is_400(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.get(f"{_base(ws, pid)}/issues/{i1}/sub-issues/?per_page=101")
    assert resp.status_code == 400
    assert resp.json() == {"detail": "Invalid per_page value. Cannot exceed 100."}
    resp = admin.get(f"{_base(ws, pid)}/issues/{i1}/sub-issues/?per_page=abc")
    assert resp.status_code == 400
    assert resp.json() == {"detail": "Invalid per_page parameter."}
    resp = admin.get(f"{_base(ws, pid)}/issues/{i1}/sub-issues/?cursor=bad")
    assert resp.status_code == 400
    assert resp.json() == {"detail": "Invalid cursor parameter."}


def test_sub_issues_page_past_the_end_is_empty(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    body = admin.get(
        f"{_base(ws, pid)}/issues/{i1}/sub-issues/?cursor=50:3:0").json()
    assert body["sub_issues"] == []
    assert body["count"] == 0
    assert body["total_count"] == 1
    assert body["prev_page_results"] is True
    # The rollup still covers the whole set.
    assert body["state_distribution"] == {"backlog": [seed["issue2"]]}


SERVER_ERROR = {"error": "Something went wrong please try again later"}


def test_sub_issues_paging_edges(clients, seed):
    admin = clients["admin"]
    ws, pid, i1, i3 = (
        seed["ws_slug"], seed["project"], seed["issue1"], seed["issue3"])
    url = f"{_base(ws, pid)}/issues/{i1}/sub-issues/"
    # An empty order_by reaches the paginator as F(""): FieldError.
    resp = admin.get(f"{url}?order_by=")
    assert resp.status_code == 500
    assert resp.json() == SERVER_ERROR
    # A negative page is BadPaginationError; a zero/negative per_page and a
    # backwards cursor with a foreign page size fail on the queryset.
    resp = admin.get(f"{url}?cursor=50:-1:0")
    assert resp.status_code == 400
    assert resp.json() == {"detail": "Error in parsing"}
    for query in ("per_page=0", "per_page=-1", "cursor=7:1:1"):
        resp = admin.get(f"{url}?{query}")
        assert resp.status_code == 500, query
        assert resp.json() == SERVER_ERROR
    # An unknown group_by key only raises when the page has a row to index.
    resp = admin.get(f"{url}?group_by=nope")
    assert resp.status_code == 400
    assert resp.json() == {"error": "The required key does not exist."}
    body = admin.get(f"{url}?group_by=nope&cursor=50:3:0").json()
    assert body["sub_issues"] == {}
    assert body["count"] == 0
    body = admin.get(
        f"{_base(ws, pid)}/issues/{i3}/sub-issues/?group_by=nope").json()
    assert body["sub_issues"] == {}
    assert body["total_pages"] == 0
    assert body["state_distribution"] == {}
