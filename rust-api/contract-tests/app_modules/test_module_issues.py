"""Module ↔ issue links (PIDASHCONV-86, D-28).

Routes (apps/api/pi_dash/app/urls/module.py):
  POST /api/workspaces/<slug>/projects/<project_id>/issues/<issue_id>/modules/
  POST /api/workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/
  GET  /api/workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/
  GET/PUT/PATCH/DELETE .../modules/<module_id>/issues/<issue_id>/

Port-existing bugs (pinned, see PR): GET/PUT/PATCH on the detail route
500 with {"error": "Something went wrong please try again later"} — the
URL names the kwargs module_id/issue_id but DRF's default
retrieve/update look for `pk`. DELETE is a custom action and works.
"""

import datetime
import json

import pytest

from .conftest import module_url, modules_url

SERVER_ERROR = {"error": "Something went wrong please try again later"}

# Paginated issue envelope (BasePaginator) for the module-issues list.
LIST_KEYS = {
    "grouped_by", "sub_grouped_by", "total_count", "next_cursor",
    "prev_cursor", "next_page_results", "prev_page_results", "count",
    "total_pages", "total_results", "extra_stats", "results",
}

ISSUE_KEYS = {
    "id", "name", "state_id", "sort_order", "completed_at",
    "estimate_point", "priority", "start_date", "target_date",
    "sequence_id", "project_id", "parent_id", "created_at", "updated_at",
    "created_by", "updated_by", "is_draft", "archived_at", "state__group",
    "cycle_id", "link_count", "attachment_count", "sub_issues_count",
    "assignee_ids", "label_ids", "module_ids",
}


def _issue_modules_url(tenant, project_id, issue_id):
    return (
        f"/api/workspaces/{tenant['workspace']['slug']}"
        f"/projects/{project_id}/issues/{issue_id}/modules/"
    )


def _module_issues_url(tenant, project_id, module_id):
    return f"{modules_url(tenant, project_id)}{module_id}/issues/"


def _module_issue_url(tenant, project_id, module_id, issue_id):
    return f"{_module_issues_url(tenant, project_id, module_id)}{issue_id}/"


def test_create_module_issues(api, seed, tenant_a, project_a, auth_a, module_a, issue_a):
    r = api.post(
        _module_issues_url(tenant_a, project_a, module_a), headers=auth_a,
        json={"issues": [issue_a]},
    )
    assert r.status_code == 201, r.text
    assert r.json() == {"message": "success"}
    with seed.conn.cursor() as cur:
        cur.execute(
            "SELECT COUNT(*) FROM module_issues WHERE module_id = %s AND issue_id = %s"
            " AND deleted_at IS NULL",
            (module_a, issue_a),
        )
        assert cur.fetchone()[0] == 1


def test_create_module_issues_empty_rejected(api, tenant_a, project_a, auth_a, module_a):
    r = api.post(
        _module_issues_url(tenant_a, project_a, module_a), headers=auth_a,
        json={},
    )
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "Issues are required"}


def test_create_issue_modules(api, seed, tenant_a, project_a, auth_a, module_a, issue_a):
    r = api.post(
        _issue_modules_url(tenant_a, project_a, issue_a), headers=auth_a,
        json={"modules": [module_a], "removed_modules": []},
    )
    assert r.status_code == 201, r.text
    assert r.json() == {"message": "success"}
    with seed.conn.cursor() as cur:
        cur.execute(
            "SELECT COUNT(*) FROM module_issues WHERE module_id = %s AND issue_id = %s"
            " AND deleted_at IS NULL",
            (module_a, issue_a),
        )
        assert cur.fetchone()[0] == 1


def test_create_issue_modules_remove(api, seed, tenant_a, project_a, auth_a, module_a, issue_a):
    seed.module_issue(tenant_a["workspace"]["id"], project_a, module_a, issue_a)
    r = api.post(
        _issue_modules_url(tenant_a, project_a, issue_a), headers=auth_a,
        json={"modules": [], "removed_modules": [module_a]},
    )
    assert r.status_code == 201, r.text
    assert r.json() == {"message": "success"}
    r = api.get(_module_issues_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 200, r.text
    assert r.json()["total_count"] == 0


def test_list_shape(api, seed, tenant_a, project_a, auth_a, module_a, issue_a):
    seed.module_issue(tenant_a["workspace"]["id"], project_a, module_a, issue_a)
    r = api.get(_module_issues_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == LIST_KEYS, sorted(set(body) ^ LIST_KEYS)
    assert body["total_count"] == 1
    row = body["results"][0]
    assert set(row) == ISSUE_KEYS, sorted(set(row) ^ ISSUE_KEYS)
    assert row["id"] == issue_a
    assert row["module_ids"] == [module_a]


def test_detail_get_put_patch_are_server_errors(api, seed, tenant_a, project_a, auth_a,
                                                module_a, issue_a):
    """Port-existing bug: the detail routes name their kwargs
    module_id/issue_id, so DRF's default retrieve/update cannot find `pk`
    and raise into the generic 500 handler."""
    seed.module_issue(tenant_a["workspace"]["id"], project_a, module_a, issue_a)
    url = _module_issue_url(tenant_a, project_a, module_a, issue_a)
    r = api.get(url, headers=auth_a)
    assert r.status_code == 500, r.text
    assert r.json() == SERVER_ERROR
    r = api.put(url, headers=auth_a, json={"name": "x"})
    assert r.status_code == 500, r.text
    assert r.json() == SERVER_ERROR
    r = api.patch(url, headers=auth_a, json={"name": "x"})
    assert r.status_code == 500, r.text
    assert r.json() == SERVER_ERROR


def test_destroy(api, seed, tenant_a, project_a, auth_a, module_a, issue_a):
    seed.module_issue(tenant_a["workspace"]["id"], project_a, module_a, issue_a)
    r = api.delete(_module_issue_url(tenant_a, project_a, module_a, issue_a), headers=auth_a)
    assert r.status_code == 204, r.text
    r = api.get(_module_issues_url(tenant_a, project_a, module_a), headers=auth_a)
    assert r.status_code == 200, r.text
    assert r.json()["total_count"] == 0


def test_requires_auth(api, tenant_a, project_a, module_a):
    r = api.get(_module_issues_url(tenant_a, project_a, module_a))
    assert r.status_code == 401, r.text


def test_module_issues_state_id_in_filter(
        api, seed, tenant_a, project_a, auth_a, module_a):
    # PIDASHCONV-1317: the module-issues list runs the same rich-filter
    # compiler; UUID lists must bind typed (pre-fix Rust 500d).
    ws = tenant_a["workspace"]["id"]
    state_a = seed.state(project_a, ws, name="Mod state A", group="unstarted")
    state_b = seed.state(project_a, ws, name="Mod state B", group="started")
    issue_a = seed.issue(ws, project_a, name="Mod filter A", state_id=state_a)
    issue_b = seed.issue(ws, project_a, name="Mod filter B", state_id=state_b)
    seed.module_issue(ws, project_a, module_a, issue_a)
    seed.module_issue(ws, project_a, module_a, issue_b)
    r = api.get(
        _module_issues_url(tenant_a, project_a, module_a), headers=auth_a,
        params={"filters": json.dumps({"and": [{"state_id__in": state_a}]})},
    )
    assert r.status_code == 200, r.text
    assert {row["id"] for row in r.json()["results"]} == {issue_a}
    r = api.get(
        _module_issues_url(tenant_a, project_a, module_a), headers=auth_a,
        params={"filters": json.dumps(
            {"and": [{"state_id__in": f"{state_a},{state_b}"}]})},
    )
    assert r.status_code == 200, r.text
    assert {row["id"] for row in r.json()["results"]} == {issue_a, issue_b}


# --- Spreadsheet sort keys (PIDASHCONV-1421) -------------------------------
#
# The web app sorts the module spreadsheet by these `order_by` values; the
# Rust port 500d on all five while Django 200-sorts them. The world below
# pins every behavior the fix relies on: `0`-form counts with a
# `-created_at` tiebreak, estimate keys with `NULLS LAST`, and the
# cycle-name traversal as Django's plain join — no `deleted_at` guard on
# the bridge or the cycle (the deleted Zulu bridge and the deleted AAA
# cycle both sort), fanning multi-linked issues out into identical rows.

def _sweep_sort_world(seed, workspace_id):
    """Delete sort-world side rows `Seed.cleanup` does not sweep."""
    with seed.conn.cursor() as cur:
        for table in ("issue_links", "file_assets", "cycle_issues"):
            cur.execute(
                f"DELETE FROM {table} WHERE workspace_id = %s", (workspace_id,)
            )
        cur.execute(
            "UPDATE issues SET estimate_point_id = NULL, parent_id = NULL"
            " WHERE workspace_id = %s",
            (workspace_id,),
        )
        for table in ("cycles", "estimate_points", "estimates"):
            cur.execute(
                f"DELETE FROM {table} WHERE workspace_id = %s", (workspace_id,)
            )


@pytest.fixture()
def sort_world(seed, tenant_a, project_a, module_a):
    """Five module issues with staggered `created_at` (A oldest, E newest):

    - A: 2 links, 1 attachment, 2 kids; estimate 5; live Beta link.
    - B: 0 links, 3 attachments, 0 kids; estimate 1; live Alpha link plus
      a *deleted* Zulu bridge (which still sorts).
    - C: 1 link, 0 attachments, 1 kid; no estimate; no cycle.
    - D: 0/0/0; no estimate; two live links (Alpha, Gamma).
    - E: 0/0/0; estimate 9; live Beta link plus a live bridge to the
      *deleted* AAA cycle (which still sorts).
    """
    ws_id = tenant_a["workspace"]["id"]
    user_id = tenant_a["user"]["id"]
    state_a = seed.state(project_a, ws_id, name="Todo", group="unstarted")
    state_b = seed.state(project_a, ws_id, name="Doing", group="started")
    now = datetime.datetime.now(datetime.timezone.utc)
    estimate = seed._insert(
        "estimates", name="CT1421 scale", description="", type="categories",
        last_used=False, workspace_id=ws_id, project_id=project_a,
        created_at=now, updated_at=now,
    )
    points = {}
    for key in (1, 5, 9):
        points[key] = seed._insert(
            "estimate_points", estimate_id=estimate, key=key, description="",
            value=str(key), workspace_id=ws_id, project_id=project_a,
            created_at=now, updated_at=now,
        )
    cycles = {}
    for name in ("Alpha", "Beta", "Gamma", "Zulu"):
        cycles[name] = seed._insert(
            "cycles", workspace_id=ws_id, project_id=project_a, name=name,
            description="", owned_by_id=user_id, view_props="{}",
            sort_order=65535, progress_snapshot="{}", logo_props="{}",
            timezone="UTC", version=1, created_at=now, updated_at=now,
        )
    cycles["AAA"] = seed._insert(
        "cycles", workspace_id=ws_id, project_id=project_a, name="AAA-gone",
        description="", owned_by_id=user_id, view_props="{}",
        sort_order=65535, progress_snapshot="{}", logo_props="{}",
        timezone="UTC", version=1, created_at=now, updated_at=now,
        deleted_at=now,
    )

    def _add(name, state, links, attachments, kids, estimate_key, bridges, idx):
        issue_id = seed.issue(ws_id, project_a, name=name, state_id=state)
        seed.module_issue(ws_id, project_a, module_a, issue_id)
        for num in range(links):
            seed._insert(
                "issue_links", title=f"l{num}",
                url=f"https://example.com/{name}/{num}",
                issue_id=issue_id, project_id=project_a, workspace_id=ws_id,
                metadata="{}", created_at=now, updated_at=now,
            )
        for num in range(attachments):
            seed._insert(
                "file_assets", attributes="{}", asset=f"{name}-att{num}",
                entity_type="ISSUE_ATTACHMENT", is_deleted=False,
                is_archived=False, is_uploaded=True, issue_id=issue_id,
                project_id=project_a, workspace_id=ws_id, size=10,
                created_at=now, updated_at=now,
            )
        for num in range(kids):
            kid = seed.issue(ws_id, project_a, name=f"{name}-kid{num}")
            with seed.conn.cursor() as cur:
                cur.execute(
                    "UPDATE issues SET parent_id = %s WHERE id = %s",
                    (issue_id, kid),
                )
        if estimate_key is not None:
            with seed.conn.cursor() as cur:
                cur.execute(
                    "UPDATE issues SET estimate_point_id = %s WHERE id = %s",
                    (points[estimate_key], issue_id),
                )
        for cycle_name, deleted in bridges:
            seed._insert(
                "cycle_issues", workspace_id=ws_id, project_id=project_a,
                cycle_id=cycles[cycle_name], issue_id=issue_id,
                created_at=now, updated_at=now,
                deleted_at=now if deleted else None,
            )
        stamp = now + datetime.timedelta(seconds=idx * 30)
        with seed.conn.cursor() as cur:
            cur.execute(
                "UPDATE issues SET created_at = %s, updated_at = %s WHERE id = %s",
                (stamp, stamp, issue_id),
            )
        return issue_id

    ids = {
        "sort-A": _add("sort-A", state_a, 2, 1, 2, 5, [("Beta", False)], 0),
        "sort-B": _add("sort-B", state_a, 0, 3, 0, 1,
                       [("Alpha", False), ("Zulu", True)], 1),
        "sort-C": _add("sort-C", state_b, 1, 0, 1, None, [], 2),
        "sort-D": _add("sort-D", state_b, 0, 0, 0, None,
                       [("Alpha", False), ("Gamma", False)], 3),
        "sort-E": _add("sort-E", state_a, 0, 0, 0, 9,
                       [("Beta", False), ("AAA", False)], 4),
    }
    try:
        yield {
            "ids": ids,
            "state_a": state_a,
            "state_b": state_b,
            "cycles": cycles,
        }
    finally:
        _sweep_sort_world(seed, ws_id)


# Django's flat id order per key (names stand in for ids): counts render
# `0` and break ties by `-created_at`; estimates and cycle names sort
# `NULLS LAST`; the cycle traversal fans out (8 rows for 5 issues).
FLAT_SORT_ORDERS = {
    "link_count": ["sort-E", "sort-D", "sort-B", "sort-C", "sort-A"],
    "-link_count": ["sort-A", "sort-C", "sort-E", "sort-D", "sort-B"],
    "attachment_count": ["sort-E", "sort-D", "sort-C", "sort-A", "sort-B"],
    "-attachment_count": ["sort-B", "sort-A", "sort-E", "sort-D", "sort-C"],
    "sub_issues_count": ["sort-E", "sort-D", "sort-B", "sort-C", "sort-A"],
    "-sub_issues_count": ["sort-A", "sort-C", "sort-E", "sort-D", "sort-B"],
    "estimate_point__key": ["sort-B", "sort-A", "sort-E", "sort-D", "sort-C"],
    "-estimate_point__key": ["sort-E", "sort-A", "sort-B", "sort-D", "sort-C"],
    "issue_cycle__cycle__name": [
        "sort-E", "sort-D", "sort-B", "sort-E",
        "sort-A", "sort-D", "sort-B", "sort-C",
    ],
    "-issue_cycle__cycle__name": [
        "sort-B", "sort-D", "sort-E", "sort-A",
        "sort-D", "sort-B", "sort-E", "sort-C",
    ],
}


def test_sort_keys_flat(api, tenant_a, project_a, auth_a, module_a, sort_world):
    url = _module_issues_url(tenant_a, project_a, module_a)
    for order_by, expected in FLAT_SORT_ORDERS.items():
        r = api.get(url, headers=auth_a, params={"order_by": order_by})
        assert r.status_code == 200, (order_by, r.text)
        body = r.json()
        assert [row["name"] for row in body["results"]] == expected, order_by
        assert body["count"] == len(expected), order_by
        # Flat totals stay distinct-issue counts even when rows fan out.
        assert body["total_count"] == 5, order_by
        assert body["total_results"] == 5, order_by
    # The count annotations render `0`, never `NULL`, on this list.
    r = api.get(url, headers=auth_a, params={"order_by": "link_count"})
    got = {row["name"]: row["link_count"] for row in r.json()["results"]}
    assert got == {
        "sort-A": 2, "sort-B": 0, "sort-C": 1, "sort-D": 0, "sort-E": 0,
    }


def _group_names(results, member):
    """Name order of the group bucket holding `member`."""
    for bucket in results.values():
        names = [row["name"] for row in bucket["results"]]
        if member in names:
            return names
    raise AssertionError(f"{member} not grouped")


def test_sort_keys_grouped(api, tenant_a, project_a, auth_a, module_a, sort_world):
    url = _module_issues_url(tenant_a, project_a, module_a)
    for order_by, expected in FLAT_SORT_ORDERS.items():
        r = api.get(
            url, headers=auth_a,
            params={"order_by": order_by, "group_by": "state_id"},
        )
        assert r.status_code == 200, (order_by, r.text)
        body = r.json()
        buckets = body["results"]
        assert len(buckets) == 2, (order_by, sorted(buckets))
        # Buckets keep the global order; per-group totals stay distinct.
        state_a_names = _group_names(buckets, "sort-A")
        state_b_names = _group_names(buckets, "sort-C")
        assert sorted(state_a_names) == sorted(
            n for n in expected if n in ("sort-A", "sort-B", "sort-E")
        ), order_by
        assert sorted(state_b_names) == sorted(
            n for n in expected if n in ("sort-C", "sort-D")
        ), order_by
        assert [n for n in expected if n in state_a_names] == state_a_names, order_by
        assert [n for n in expected if n in state_b_names] == state_b_names, order_by
        totals = sorted(b["total_results"] for b in buckets.values())
        assert totals == [2, 3], (order_by, totals)
        # Grouped totals count the joined rows, fan-out included.
        assert body["count"] == len(expected), order_by
        assert body["total_count"] == len(expected), order_by
        assert body["total_results"] == len(expected), order_by


def test_sort_keys_sub_grouped(api, tenant_a, project_a, auth_a, module_a, sort_world):
    url = _module_issues_url(tenant_a, project_a, module_a)
    for order_by, expected in FLAT_SORT_ORDERS.items():
        r = api.get(
            url, headers=auth_a,
            params={
                "order_by": order_by,
                "group_by": "state_id",
                "sub_group_by": "priority",
            },
        )
        assert r.status_code == 200, (order_by, r.text)
        body = r.json()
        buckets = body["results"]
        assert len(buckets) == 2, (order_by, sorted(buckets))
        # Every issue has priority `none`: one cell per group, same rows.
        for bucket in buckets.values():
            cells = bucket["results"]
            assert sorted(cells) == ["none"], (order_by, sorted(cells))
        flat: list[str] = []
        for bucket in buckets.values():
            for cell in bucket["results"].values():
                flat.extend(row["name"] for row in cell["results"])
        assert sorted(flat) == sorted(expected), order_by
        assert body["count"] == len(expected), order_by
        assert body["total_count"] == len(expected), order_by


def test_sort_keys_filtered_cycle_sort(
    api, tenant_a, project_a, auth_a, module_a, sort_world
):
    # A cycle filter lends its bridge join to the traversal (Django reuses
    # it, scope included): B's deleted Zulu bridge and D's Gamma bridge do
    # not fan out here.
    url = _module_issues_url(tenant_a, project_a, module_a)
    filters = json.dumps(
        {"and": [{"cycle_id__in": sort_world["cycles"]["Alpha"]}]}
    )
    for order_by in ("issue_cycle__cycle__name", "-issue_cycle__cycle__name"):
        r = api.get(
            url, headers=auth_a,
            params={"order_by": order_by, "filters": filters},
        )
        assert r.status_code == 200, (order_by, r.text)
        body = r.json()
        assert [row["name"] for row in body["results"]] == [
            "sort-D", "sort-B",
        ], order_by
        assert body["count"] == 2, order_by
        assert body["total_count"] == 2, order_by


def test_sort_keys_unknown_stays_server_error(
    api, tenant_a, project_a, auth_a, module_a, sort_world
):
    url = _module_issues_url(tenant_a, project_a, module_a)
    r = api.get(url, headers=auth_a, params={"order_by": "no_such_key"})
    assert r.status_code == 500, r.text
    assert r.json() == SERVER_ERROR


def test_default_order_is_created_at_ascending(
    api, seed, tenant_a, project_a, auth_a, module_a
):
    # PIDASHCONV-1422: with no order_by the module list defaults to
    # `created_at` ascending (module/issue.py:112), unlike the cycle and
    # project lists which default to `-created_at`. The Rust port used the
    # shared `-created_at` default and returned newest-first.
    ws_id = tenant_a["workspace"]["id"]
    old_id = seed.issue(ws_id, project_a, name="Mod order OLD")
    new_id = seed.issue(ws_id, project_a, name="Mod order NEW")
    seed.module_issue(ws_id, project_a, module_a, old_id)
    seed.module_issue(ws_id, project_a, module_a, new_id)
    with seed.conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET created_at = %s, updated_at = %s WHERE id = %s",
            (
                datetime.datetime(2024, 1, 1, tzinfo=datetime.timezone.utc),
                datetime.datetime(2024, 1, 1, tzinfo=datetime.timezone.utc),
                old_id,
            ),
        )
        cur.execute(
            "UPDATE issues SET created_at = %s, updated_at = %s WHERE id = %s",
            (
                datetime.datetime(2025, 6, 1, tzinfo=datetime.timezone.utc),
                datetime.datetime(2025, 6, 1, tzinfo=datetime.timezone.utc),
                new_id,
            ),
        )
    url = _module_issues_url(tenant_a, project_a, module_a)
    r = api.get(url, headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["total_count"] == 2, body
    assert [row["id"] for row in body["results"]] == [old_id, new_id], body
    r = api.get(url, headers=auth_a, params={"group_by": "priority"})
    assert r.status_code == 200, r.text
    buckets = r.json()["results"]
    assert [row["id"] for row in buckets["none"]["results"]] == [
        old_id, new_id,
    ], sorted(buckets)
