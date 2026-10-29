"""PIDASHCONV-1504: short group aliases + flat row key order on cycle-issues.

Django resolves ``group_by`` as a raw ORM path, so the short aliases
``state`` / ``labels`` / ``assignees`` answer 200: the window partitions by
``issues.state_id`` / ``label_issue.label_id`` / ``issue_assignee.assignee_id``
(LEFT joins, no deleted guard — unlike the canonical ``labels__id`` form,
which adds the ``GROUP_FILTER_MAPPER`` guard), while ``issue_group_values``
returns ``[]`` for the unknown names so no window row is ever placed and
``results`` is ``{}``. Sub-grouped combinations where neither side is a
canonical m2m field die in the sub grouper's direct cell indexing
(``KeyError`` -> 400 ``{"error": "The required key does not exist."}``);
combinations with a canonical-m2m side take the guarded multi path (200),
and an empty window answers 200 ``{}`` before the grouper runs.

The flat row key order is Django's ``.values()`` wire order (concrete
fields, then the ``state__group`` lookup, then annotations in annotation
order) — not the ``required_fields`` argument order from ``grouper.py``.
"""

import pytest

from _harness.seed import ADMIN
from .conftest import create_issue_for, cycle_urls

pytestmark = pytest.mark.contract

KEY_ERROR = {"error": "The required key does not exist."}

WIRE_ORDER = [
    "id", "name", "state_id", "sort_order", "completed_at", "estimate_point",
    "priority", "start_date", "target_date", "sequence_id", "project_id",
    "parent_id", "created_at", "updated_at", "created_by", "updated_by",
    "is_draft", "archived_at", "state__group", "cycle_id", "link_count",
    "attachment_count", "sub_issues_count", "assignee_ids", "label_ids",
    "module_ids",
]


def _seed_varied_cycle(tenant):
    """Two issues with distinct states/priorities/labels/assignees.

    i1 carries two live labels and one assignee; i2 is unlabelled except
    for one soft-deleted link (visible to the unguarded short-``labels``
    join, excluded by the canonical guard) and has no assignee.
    Returns (i1, i2, label_ids, state_ids).
    """
    seed = tenant["seed"]
    ws_id = tenant["workspace"]["id"]
    proj_id = tenant["project"]["id"]
    other = seed.user(email=f"alias-other-{tenant['cycle']['id'][:8]}@contract.test")
    seed.member(ws_id, other["id"], role=ADMIN)
    seed.project_member(proj_id, ws_id, other["id"], role=ADMIN)
    done_id = seed.state(proj_id, ws_id, name="Done", group="completed")
    todo_id = tenant["state"]["id"]
    l1 = seed.label(ws_id, proj_id, name="Alias label one")
    l2 = seed.label(ws_id, proj_id, name="Alias label two")
    i1 = create_issue_for(tenant, name="Alias issue one")
    i2 = create_issue_for(tenant, name="Alias issue two")
    with seed.conn.cursor() as cur:
        cur.execute(
            "update issues set state_id=%s, priority='urgent' where id=%s",
            (done_id, i2["id"]),
        )
    seed.issue_label(ws_id, proj_id, i1["id"], l1)
    seed.issue_label(ws_id, proj_id, i1["id"], l2)
    dead = seed.issue_label(ws_id, proj_id, i2["id"], l1)
    with seed.conn.cursor() as cur:
        cur.execute(
            "update issue_labels set deleted_at=now() where id=%s", (dead,)
        )
    seed.issue_assignee(ws_id, proj_id, i1["id"], other["id"])
    seed.conn.commit()
    urls = cycle_urls(tenant)
    r = tenant["client"].post(
        urls["cycle_issues"](tenant["cycle"]["id"]),
        json={"issues": [i1["id"], i2["id"]]},
    )
    assert r.status_code == 201, r.text[:500]
    return i1, i2, (l1, l2), (todo_id, done_id), other["id"]


def _teardown_bridges(tenant):
    """Hard-delete the tenant's bridges (Seed.cleanup cannot order them)."""
    with tenant["seed"].conn.cursor() as cur:
        cur.execute(
            "delete from cycle_issues where workspace_id=%s",
            (tenant["workspace"]["id"],),
        )
    tenant["seed"].conn.commit()


def _find_row(node, issue_id):
    if isinstance(node, dict):
        if node.get("id") == issue_id and "link_count" in node:
            return node
        for value in node.values():
            hit = _find_row(value, issue_id)
            if hit is not None:
                return hit
    elif isinstance(node, list):
        for item in node:
            hit = _find_row(item, issue_id)
            if hit is not None:
                return hit
    return None


def test_flat_row_key_order_matches_django(make_tenant):
    tenant = make_tenant(role=ADMIN)
    try:
        _seed_varied_cycle(tenant)
        urls = cycle_urls(tenant)
        r = tenant["client"].get(urls["cycle_issues"](tenant["cycle"]["id"]))
        assert r.status_code == 200, r.text[:500]
        rows = r.json()["results"]
        assert len(rows) == 2
        for row in rows:
            assert list(row.keys()) == WIRE_ORDER, list(row.keys())
    finally:
        _teardown_bridges(tenant)


@pytest.mark.parametrize(
    "group_by,joined",
    [("state", 2), ("labels", 3), ("assignees", 2)],
)
def test_short_alias_single_group_matches_django(make_tenant, group_by, joined):
    """Short aliases 200 with empty results; counts follow the joined rows.

    ``labels`` counts the soft-deleted link (unguarded LEFT join):
    i1 x 2 + i2 x 1 = 3. Grouped totals are the windowed joined count
    (``queryset.count()``), never distinct.
    """
    tenant = make_tenant(role=ADMIN)
    try:
        _seed_varied_cycle(tenant)
        urls = cycle_urls(tenant)
        r = tenant["client"].get(
            urls["cycle_issues"](tenant["cycle"]["id"]) + f"?group_by={group_by}"
        )
        assert r.status_code == 200, r.text[:500]
        assert r.json() == {
            "grouped_by": group_by,
            "sub_grouped_by": None,
            "total_count": joined,
            "next_cursor": "1000:1:0",
            "prev_cursor": "1000:-1:1",
            "next_page_results": False,
            "prev_page_results": False,
            "count": joined,
            "total_pages": 1,
            "total_results": joined,
            "extra_stats": None,
            "results": {},
        }
    finally:
        _teardown_bridges(tenant)


@pytest.mark.parametrize(
    "group_by,sub_group_by",
    [
        ("state", "priority"),
        ("labels", "priority"),
        ("assignees", "priority"),
        ("priority", "state"),
        ("priority", "labels"),
        ("priority", "assignees"),
        ("state", "state_id"),
        ("state", "labels"),
        ("labels", "assignees"),
    ],
)
def test_short_alias_sub_combo_is_key_error(make_tenant, group_by, sub_group_by):
    """Sub combos without a canonical-m2m side miss the seeded cells."""
    tenant = make_tenant(role=ADMIN)
    try:
        _seed_varied_cycle(tenant)
        urls = cycle_urls(tenant)
        r = tenant["client"].get(
            urls["cycle_issues"](tenant["cycle"]["id"])
            + f"?group_by={group_by}&sub_group_by={sub_group_by}"
        )
        assert r.status_code == 400, (group_by, sub_group_by, r.text[:300])
        assert r.json() == KEY_ERROR
    finally:
        _teardown_bridges(tenant)


def test_canonical_group_short_sub_takes_guarded_path(make_tenant):
    """labels__id x state: 200; cells seeded from observed pairs, rows miss."""
    tenant = make_tenant(role=ADMIN)
    try:
        _seed_varied_cycle(tenant)
        urls = cycle_urls(tenant)
        r = tenant["client"].get(
            urls["cycle_issues"](tenant["cycle"]["id"])
            + "?group_by=labels__id&sub_group_by=state"
        )
        assert r.status_code == 200, r.text[:500]
        body = r.json()
        assert body["count"] == 2, body
        assert sorted(body["results"]) == sorted(_labels_of(tenant) + ["None"]), body
        for name, cell in body["results"].items():
            if name == "None":
                assert cell["total_results"] == 0, cell
                assert cell["results"] == {}, cell
                continue
            assert cell["total_results"] == 1, cell
            assert cell["results"], "sub cells seeded from observed pairs"
            for sub in cell["results"].values():
                assert sub["results"] == [], sub
    finally:
        _teardown_bridges(tenant)


def _labels_of(tenant):
    with tenant["seed"].conn.cursor() as cur:
        cur.execute(
            "select id from labels where project_id=%s and deleted_at is null",
            (tenant["project"]["id"],),
        )
        return [str(row[0]) for row in cur.fetchall()]


def test_short_group_canonical_sub_is_empty(make_tenant):
    """state x labels__id: 200 with {} (no declared groups to seed)."""
    tenant = make_tenant(role=ADMIN)
    try:
        _seed_varied_cycle(tenant)
        urls = cycle_urls(tenant)
        r = tenant["client"].get(
            urls["cycle_issues"](tenant["cycle"]["id"])
            + "?group_by=state&sub_group_by=labels__id"
        )
        assert r.status_code == 200, r.text[:500]
        body = r.json()
        assert body["count"] == 2, body
        assert body["results"] == {}, body
    finally:
        _teardown_bridges(tenant)


def test_canonical_labels_uses_guarded_left_join(make_tenant):
    """labels__id keeps linkless issues as None and drops dead-only links.

    i2's only link is soft-deleted, so i2 vanishes (no None bucket here);
    i1 lands in both live buckets with the swapped row shape.
    """
    tenant = make_tenant(role=ADMIN)
    try:
        i1, i2, (l1, l2), _, _ = _seed_varied_cycle(tenant)
        urls = cycle_urls(tenant)
        r = tenant["client"].get(
            urls["cycle_issues"](tenant["cycle"]["id"]) + "?group_by=labels__id"
        )
        assert r.status_code == 200, r.text[:500]
        body = r.json()
        assert body["count"] == 2, body
        assert sorted(body["results"]) == sorted([l1, l2]), body
        for bucket in body["results"].values():
            assert bucket["total_results"] == 1, bucket
            assert [row["id"] for row in bucket["results"]] == [i1["id"]]
        row = body["results"][l1]["results"][0]
        assert list(row.keys()) == [
            "id", "name", "state_id", "sort_order", "completed_at",
            "estimate_point", "priority", "start_date", "target_date",
            "sequence_id", "project_id", "parent_id", "created_at",
            "updated_at", "created_by", "updated_by", "is_draft",
            "archived_at", "state__group", "labels__id", "cycle_id",
            "link_count", "attachment_count", "sub_issues_count",
            "assignee_ids", "module_ids", "label_ids",
        ], list(row.keys())
        # The fanned row keeps whichever joined label row came first
        # (same-issue rows tie on the window order on both backends).
        assert row["labels__id"] in (l1, l2), row
        assert sorted(row["label_ids"]) == sorted([l1, l2]), row
        assert _find_row(body["results"], i2["id"]) is None
    finally:
        _teardown_bridges(tenant)


def test_canonical_assignees_keeps_none_bucket(make_tenant):
    tenant = make_tenant(role=ADMIN)
    try:
        i1, i2, _, _, assignee_id = _seed_varied_cycle(tenant)
        urls = cycle_urls(tenant)
        r = tenant["client"].get(
            urls["cycle_issues"](tenant["cycle"]["id"]) + "?group_by=assignees__id"
        )
        assert r.status_code == 200, r.text[:500]
        body = r.json()
        assert body["count"] == 2, body
        assert sorted(body["results"]) == sorted([assignee_id, "None"]), body
        assert [row["id"] for row in body["results"][assignee_id]["results"]] == [
            i1["id"]
        ]
        none_rows = body["results"]["None"]["results"]
        assert [row["id"] for row in none_rows] == [i2["id"]]
        assert none_rows[0]["assignee_ids"] == [], none_rows[0]
    finally:
        _teardown_bridges(tenant)


def test_canonical_double_swap_row_order(make_tenant):
    """labels__id x assignees__id pins the swapped lookup + append order."""
    tenant = make_tenant(role=ADMIN)
    try:
        i1, _, (l1, l2), _, assignee_id = _seed_varied_cycle(tenant)
        urls = cycle_urls(tenant)
        r = tenant["client"].get(
            urls["cycle_issues"](tenant["cycle"]["id"])
            + "?group_by=labels__id&sub_group_by=assignees__id"
        )
        assert r.status_code == 200, r.text[:500]
        body = r.json()
        assert body["count"] == 2, body
        row = _find_row(body["results"], i1["id"])
        assert row is not None, body
        assert list(row.keys()) == [
            "id", "name", "state_id", "sort_order", "completed_at",
            "estimate_point", "priority", "start_date", "target_date",
            "sequence_id", "project_id", "parent_id", "created_at",
            "updated_at", "created_by", "updated_by", "is_draft",
            "archived_at", "state__group", "labels__id", "assignees__id",
            "cycle_id", "link_count", "attachment_count", "sub_issues_count",
            "module_ids", "label_ids", "assignee_ids",
        ], list(row.keys())
        assert sorted(row["label_ids"]) == sorted([l1, l2]), row
        assert row["assignee_ids"] == [assignee_id], row
    finally:
        _teardown_bridges(tenant)


def test_empty_cycle_short_combos_are_empty(make_tenant):
    tenant = make_tenant(role=ADMIN)
    urls = cycle_urls(tenant)
    base = urls["cycle_issues"](tenant["cycle"]["id"])
    r = tenant["client"].get(base + "?group_by=state")
    assert r.status_code == 200, r.text[:500]
    assert r.json()["results"] == {}
    assert r.json()["count"] == 0
    r = tenant["client"].get(base + "?group_by=state&sub_group_by=priority")
    assert r.status_code == 200, r.text[:500]
    assert r.json()["results"] == {}
    assert r.json()["count"] == 0
