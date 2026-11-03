"""PIDASHCONV-1416: a bare issue in a cycle renders its counts as 0, not null.

Django annotates the cycle-issue list with the scalar
``Func(F("id"), function="Count")`` shape
(``app/views/cycle/issue.py:84-104``), whose subquery always returns one
row, so an issue with no links, attachments or sub-issues renders
``link_count == attachment_count == sub_issues_count == 0`` on the flat,
grouped and sub-grouped list paths. The Rust port must use plain
``COUNT(*)``, never ``NULLIF(COUNT(*), 0)``, for these three counts.
"""

import uuid
from datetime import datetime, timezone

import pytest

from .conftest import create_issue_for, cycle_urls

pytestmark = pytest.mark.contract


def _add_link(tenant, issue_id, url="https://example.com/spec"):
    """Seed one ``issue_links`` row straight into Postgres (``Seed`` has no
    link factory; this mirrors the columns ``Seeder.create_issue_link``
    writes)."""
    link_id = str(uuid.uuid4())
    now = datetime.now(timezone.utc)
    with tenant["seed"].conn.cursor() as cur:
        cur.execute(
            """insert into issue_links
               (id, title, url, issue_id, project_id, workspace_id,
                metadata, created_at, updated_at)
               values (%s,'Contract link',%s,%s,%s,%s,'{}',%s,%s)""",
            (
                link_id,
                url,
                issue_id,
                tenant["project"]["id"],
                tenant["workspace"]["id"],
                now,
                now,
            ),
        )
    tenant["seed"].track("issue_links", "id", link_id)
    return {"id": link_id}


def _find_row(node, issue_id):
    """Locate the issue's row in a flat, grouped or sub-grouped payload."""
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


def _row_for(client, url, issue_id, shape):
    r = client.get(url)
    assert r.status_code == 200, (shape, r.status_code, r.text[:500])
    row = _find_row(r.json(), issue_id)
    assert row is not None, (shape, "row missing", r.text[:800])
    return row


def test_bare_issue_counts_are_zero_on_all_list_shapes(admin):
    urls = cycle_urls(admin)
    pk = admin["cycle"]["id"]
    issue = create_issue_for(admin, name="Bare counts issue")
    try:
        r = admin["client"].post(
            urls["cycle_issues"](pk), json={"issues": [issue["id"]]}
        )
        assert r.status_code == 201, r.text[:500]
        base = urls["cycle_issues"](pk)
        for shape, url in [
            ("flat", base),
            ("grouped", base + "?group_by=state_id"),
            ("sub-grouped", base + "?group_by=state_id&sub_group_by=priority"),
        ]:
            row = _row_for(admin["client"], url, issue["id"], shape)
            assert row["link_count"] == 0, (shape, row)
            assert row["attachment_count"] == 0, (shape, row)
            assert row["sub_issues_count"] == 0, (shape, row)
    finally:
        admin["client"].delete(urls["cycle_issue_detail"](pk, issue["id"]))


def test_link_count_increments_after_adding_a_link(admin):
    urls = cycle_urls(admin)
    pk = admin["cycle"]["id"]
    issue = create_issue_for(admin, name="Linked counts issue")
    try:
        r = admin["client"].post(
            urls["cycle_issues"](pk), json={"issues": [issue["id"]]}
        )
        assert r.status_code == 201, r.text[:500]
        _add_link(admin, issue["id"])
        row = _row_for(admin["client"], urls["cycle_issues"](pk), issue["id"], "flat")
        assert row["link_count"] == 1, row
        assert row["attachment_count"] == 0, row
        assert row["sub_issues_count"] == 0, row
    finally:
        admin["client"].delete(urls["cycle_issue_detail"](pk, issue["id"]))
