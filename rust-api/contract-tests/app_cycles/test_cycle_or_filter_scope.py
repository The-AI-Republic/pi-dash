"""NEWPIDASH-296: cycle work-item list with a rich `or` filter must keep scope.

The Rust `cycle_filtered_set` splices the rendered rich fragment (`A OR B`)
unparenthesised into the `WHERE` chain joined with `AND`, so `scope AND A OR B`
parses as `(scope AND A) OR B` and the second disjunct leaks rows from other
workspaces/projects/cycles. Django's `.filter(Q(...))` parenthesises the tree.

Seed: CYC holds one work item A (urgent); B has label L2 but is not in the cycle.
A second workspace holds urgent C in its own cycle (so it has a bridge row).
GET cycle-issues with `or (label L2, priority urgent)` must return only A.

Django (oracle) returns count=1, unaffected Rust before the fix returns
count=2+ (including C cross-workspace). After the fix both return count=1.
"""

import json

import pytest

from _harness.seed import ADMIN
from .conftest import create_issue_for, cycle_urls

pytestmark = pytest.mark.contract


def _get_ids(resp):
    assert resp.status_code == 200, resp.text[:800]
    body = resp.json()
    return {row["id"] for row in body["results"]}, body


def test_or_filter_does_not_leak_cross_workspace(make_tenant):
    """Cross-workspace leak: second workspace urgent must not appear."""
    tenant = make_tenant(role=ADMIN)
    tenant2 = make_tenant(role=ADMIN)
    try:
        seed = tenant["seed"]
        ws_id = tenant["workspace"]["id"]
        proj_id = tenant["project"]["id"]
        cycle_id = tenant["cycle"]["id"]

        # Label L2 in first project
        l2 = seed.label(ws_id, proj_id, name="NEWPIDASH-296 L2")

        # A: urgent, in cycle. B: label L2, NOT in cycle, not urgent.
        a = create_issue_for(tenant, name="296 A urgent in cycle")
        b = create_issue_for(tenant, name="296 B label L2 not in cycle")
        with seed.conn.cursor() as cur:
            cur.execute(
                "update issues set priority='urgent' where id=%s", (a["id"],)
            )
            # B stays 'high' (not urgent)
        seed.issue_label(ws_id, proj_id, b["id"], l2)

        urls = cycle_urls(tenant)
        r = tenant["client"].post(
            urls["cycle_issues"](cycle_id),
            json={"issues": [a["id"]]},
        )
        assert r.status_code == 201, r.text[:500]

        # Second workspace: urgent C in its OWN cycle (bridge row exists)
        seed2 = tenant2["seed"]
        cycle2_id = tenant2["cycle"]["id"]
        c = create_issue_for(tenant2, name="296 C cross-ws urgent")
        with seed2.conn.cursor() as cur:
            cur.execute(
                "update issues set priority='urgent' where id=%s", (c["id"],)
            )
        urls2 = cycle_urls(tenant2)
        r2 = tenant2["client"].post(
            urls2["cycle_issues"](cycle2_id),
            json={"issues": [c["id"]]},
        )
        assert r2.status_code == 201, r2.text[:500]

        # The probe filter: label L2 OR priority urgent
        query = {
            "or": [
                {"label_id__in": [str(l2)]},
                {"priority__in": ["urgent"]},
            ]
        }
        resp = tenant["client"].get(
            urls["cycle_issues"](cycle_id),
            params={"filters": json.dumps(query)},
        )
        ids, body = _get_ids(resp)
        # Only A is in the cycle and matches the or (urgent). B is not in the cycle.
        # C is urgent but in another workspace/cycle — must not leak.
        assert ids == {a["id"]}, f"expected only cycle's A, got {ids}"
        assert c["id"] not in ids, "cross-workspace urgent leaked"
        assert b["id"] not in ids, "non-cycle label issue leaked"
        assert body["count"] == 1, body
        assert body["total_count"] == 1, body
        assert body["total_results"] == 1, body

        # Sanity: unfiltered list also only A
        resp2 = tenant["client"].get(urls["cycle_issues"](cycle_id))
        ids2, body2 = _get_ids(resp2)
        assert ids2 == {a["id"]}, ids2
    finally:
        # API-created bridges are not tracked by Seed._insert
        for t in (tenant, tenant2):
            with t["seed"].conn.cursor() as cur:
                cur.execute(
                    "delete from cycle_issues where workspace_id=%s",
                    (t["workspace"]["id"],),
                )
            t["seed"].conn.commit()


def test_or_filter_priority_alone_is_scoped(make_tenant):
    """Priority-only or (second disjunct) must also stay scoped.

    ``or`` with a single urgent disjunct would still leak if unparenthesised
    with a second disjunct that adds a label branch; here we cover the pure
    urgent branch with an empty first disjunct check via label non-match.
    """
    tenant = make_tenant(role=ADMIN)
    tenant2 = make_tenant(role=ADMIN)
    try:
        seed = tenant["seed"]
        ws_id = tenant["workspace"]["id"]
        proj_id = tenant["project"]["id"]
        cycle_id = tenant["cycle"]["id"]

        # Label L2 present but B not in cycle; filter still leaks via urgent.
        l2 = seed.label(ws_id, proj_id, name="NEWPIDASH-296 L2b")
        a = create_issue_for(tenant, name="296 A2 urgent in cycle")
        with seed.conn.cursor() as cur:
            cur.execute(
                "update issues set priority='urgent' where id=%s", (a["id"],)
            )
        urls = cycle_urls(tenant)
        r = tenant["client"].post(
            urls["cycle_issues"](cycle_id),
            json={"issues": [a["id"]]},
        )
        assert r.status_code == 201, r.text[:500]

        seed2 = tenant2["seed"]
        cycle2_id = tenant2["cycle"]["id"]
        c = create_issue_for(tenant2, name="296 C2 cross-ws urgent")
        with seed2.conn.cursor() as cur:
            cur.execute(
                "update issues set priority='urgent' where id=%s", (c["id"],)
            )
        urls2 = cycle_urls(tenant2)
        r2 = tenant2["client"].post(
            urls2["cycle_issues"](cycle2_id),
            json={"issues": [c["id"]]},
        )
        assert r2.status_code == 201, r2.text[:500]

        # Use a label that matches nothing in the cycle (L2 is not on A)
        # but priority urgent matches A only after fix.
        query = {
            "or": [
                {"label_id__in": [str(l2)]},
                {"priority__in": ["urgent"]},
            ]
        }
        resp = tenant["client"].get(
            urls["cycle_issues"](cycle_id),
            params={"filters": json.dumps(query)},
        )
        ids, body = _get_ids(resp)
        assert ids == {a["id"]}, ids
        assert c["id"] not in ids
        assert body["count"] == 1
    finally:
        for t in (tenant, tenant2):
            with t["seed"].conn.cursor() as cur:
                cur.execute(
                    "delete from cycle_issues where workspace_id=%s",
                    (t["workspace"]["id"],),
                )
            t["seed"].conn.commit()
