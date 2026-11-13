"""NEWPIDASH-219 (PB-0097, PB-0109): archived counts exclude removed bridges.

Removing a work item from a cycle (``DELETE .../cycle-issues/<id>/``, which
soft-deletes the ``cycle_issues`` bridge) and then archiving the cycle must
report the live counts on the archived list/detail, not the pre-removal
totals. Django over-counts (ported bug 5); the Rust arm asserts the fixed,
live-matching counts via ``expect("DIFF-19", ...)`` while the Django arm
pins the old over-count (see ``.ai_design/validation/differences.md``).
"""

from datetime import datetime, timedelta, timezone

import pytest

from _harness.diffs import expect

from .conftest import create_issue_for, cycle_urls

pytestmark = pytest.mark.contract


def _past_range(days_ago_start=30, days_ago_end=1):
    now = datetime.now(timezone.utc).date()
    return (
        (now - timedelta(days=days_ago_start)).isoformat(),
        (now - timedelta(days=days_ago_end)).isoformat(),
    )


def _set_state(tenant, issue_id, state_id, completed_at=None):
    with tenant["seed"].conn.cursor() as cur:
        if completed_at is None:
            cur.execute(
                "UPDATE issues SET state_id=%s WHERE id=%s", (state_id, issue_id)
            )
        else:
            cur.execute(
                "UPDATE issues SET state_id=%s, completed_at=%s WHERE id=%s",
                (state_id, completed_at, issue_id),
            )


def _cleanup_cycle(tenant, cycle_id):
    with tenant["seed"].conn.cursor() as cur:
        cur.execute("DELETE FROM cycle_issues WHERE cycle_id = %s", (cycle_id,))
        cur.execute("DELETE FROM cycles WHERE id = %s", (cycle_id,))


def _close_cycle(tenant, cycle_id):
    """Move the cycle's end date into the past so it can be archived.

    Issues are added while the cycle is still active: a completed cycle
    refuses new cycle-issue adds (``POST .../cycle-issues/`` 400), while
    removal and archiving still work on it (dbg probe DONE2, 204/200).
    """
    _, end = _past_range(30, 1)
    with tenant["seed"].conn.cursor() as cur:
        cur.execute("UPDATE cycles SET end_date = %s WHERE id = %s", (end, cycle_id))


def _archived_row(client, urls, pk):
    r = client.get(urls["archived"])
    assert r.status_code == 200, r.text[:500]
    rows = {row["id"]: row for row in r.json()}
    assert pk in rows, r.text[:800]
    return rows[pk]


def test_archived_list_and_detail_exclude_removed_bridge(make_tenant):
    tenant = make_tenant()
    urls = cycle_urls(tenant)
    seed = tenant["seed"]
    ws = tenant["workspace"]["id"]
    project = tenant["project"]["id"]
    backlog = seed.state(project, ws, name="Backlog", group="backlog")
    done_state = seed.state(project, ws, name="Done", group="completed")
    completed_at = datetime.now(timezone.utc) - timedelta(days=10)

    start, _ = _past_range(30, 1)
    future = (datetime.now(timezone.utc).date() + timedelta(days=30)).isoformat()
    r = tenant["client"].post(
        urls["cycles"],
        json={"name": "Archive counts", "start_date": start, "end_date": future},
    )
    assert r.status_code == 201, r.text[:500]
    pk = r.json()["id"]
    try:
        keep = create_issue_for(tenant, name="Kept backlog")
        drop = create_issue_for(tenant, name="Dropped completed")
        _set_state(tenant, keep["id"], backlog)
        _set_state(tenant, drop["id"], done_state, completed_at=completed_at)

        r = tenant["client"].post(
            urls["cycle_issues"](pk), json={"issues": [keep["id"], drop["id"]]}
        )
        assert r.status_code == 201, r.text[:500]

        _close_cycle(tenant, pk)

        r = tenant["client"].delete(urls["cycle_issue_detail"](pk, drop["id"]))
        assert r.status_code == 204, r.text[:500]

        # The live detail already reports the removal on both backends.
        r = tenant["client"].get(urls["detail"](pk))
        assert r.status_code == 200, r.text[:500]
        assert r.json()["total_issues"] == 1, r.text[:500]
        assert r.json()["completed_issues"] == 0, r.text[:500]

        r = tenant["client"].post(urls["archive"](pk))
        assert r.status_code == 200, r.text[:500]

        row = _archived_row(tenant["client"], urls, pk)
        assert row["total_issues"] == expect("DIFF-19", django=2, rust=1), row
        assert row["completed_issues"] == expect("DIFF-19", django=1, rust=0), row
        assert row["cancelled_issues"] == 0, row

        r = tenant["client"].get(urls["archived_detail"](pk))
        assert r.status_code == 200, r.text[:500]
        body = r.json()
        assert body["total_issues"] == expect("DIFF-19", django=2, rust=1), body
        assert body["completed_issues"] == expect("DIFF-19", django=1, rust=0), body

        # The issues completion chart's total follows total_issues.
        chart = body["distribution"]["completion_chart"]
        assert chart, body
        assert {k: v for k, v in chart.items() if v is not None}, body
        last = chart[max(chart)]
        assert last == expect("DIFF-19", django=2, rust=1), chart
    finally:
        _cleanup_cycle(tenant, pk)


def test_archived_assignee_ids_exclude_soft_deleted_assignee_bridge(make_tenant):
    tenant = make_tenant()
    urls = cycle_urls(tenant)
    seed = tenant["seed"]
    ws = tenant["workspace"]["id"]
    project = tenant["project"]["id"]
    user_id = tenant["user"]["id"]

    start, _ = _past_range(30, 1)
    future = (datetime.now(timezone.utc).date() + timedelta(days=30)).isoformat()
    r = tenant["client"].post(
        urls["cycles"],
        json={"name": "Archive assignees", "start_date": start, "end_date": future},
    )
    assert r.status_code == 201, r.text[:500]
    pk = r.json()["id"]
    try:
        issue = create_issue_for(tenant, name="Assigned issue")
        seed.issue_assignee(ws, project, issue["id"], user_id)

        r = tenant["client"].post(
            urls["cycle_issues"](pk), json={"issues": [issue["id"]]}
        )
        assert r.status_code == 201, r.text[:500]

        _close_cycle(tenant, pk)

        # Soft-delete the assignee bridge; the cycle bridge stays live.
        with seed.conn.cursor() as cur:
            cur.execute(
                "UPDATE issue_assignees SET deleted_at = now() WHERE issue_id = %s",
                (issue["id"],),
            )

        # The live detail excludes the soft-deleted assignee on both backends.
        r = tenant["client"].get(urls["detail"](pk))
        assert r.status_code == 200, r.text[:500]
        assert user_id not in {str(x) for x in r.json()["assignee_ids"]}, r.text[:500]

        r = tenant["client"].post(urls["archive"](pk))
        assert r.status_code == 200, r.text[:500]

        row = _archived_row(tenant["client"], urls, pk)
        assignees = {str(x) for x in row["assignee_ids"]}
        assert assignees == expect("DIFF-19", django={user_id}, rust=set()), row
    finally:
        _cleanup_cycle(tenant, pk)
