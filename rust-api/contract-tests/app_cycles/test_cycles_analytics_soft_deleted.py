"""NEWPIDASH-220 (DIFF-21, PB-0102): distributions exclude soft-deleted bridges.

Django groups ``GET cycles/<cid>/analytics/?type=issues|points`` through
``F("assignees__*")`` / ``F("labels__*")`` with no
``issue_assignees__deleted_at__isnull=True`` guard (``base.py:843-1033``),
so after a removal (``PATCH issues/<id>/ {"assignee_ids": []}`` -> 204,
``deleted_at`` set) the removed assignee/label keeps its distribution row
with an inflated total and the issue never lands in the NULL (no-assignee/
no-label) group — e.g. Bot assigned only to B is credited with 2 while
``assignee_ids`` (which does guard, ``base.py:174``) lists Bot only via B.
The archived detail (``archive.py:367-567``) and the transfer snapshot
(``cycle_transfer_issues.py:164-384``) share the gap.

Rust now carries ``ia.deleted_at IS NULL`` / ``il.deleted_at IS NULL`` on
every distribution LEFT JOIN (``handlers_analytics.rs``,
``handlers_archive.rs``, ``handlers_misc.rs``). These cases pin both arms:
Django keeps the over-count, Rust counts live-only (DIFF-21). Before the
fix the Rust arm fails (Rust still over-counts); after, it passes.
"""

import uuid
from datetime import datetime, timedelta, timezone

import pytest

from _harness.diffs import expect
from .conftest import cycle_urls

pytestmark = pytest.mark.contract


def _past_range(days_ago_start=40, days_ago_end=5):
    now = datetime.now(timezone.utc)
    return (now - timedelta(days=days_ago_start), now - timedelta(days=days_ago_end))


def _future_range(days_ahead_start=0, days_ahead_end=20):
    now = datetime.now(timezone.utc)
    return (now + timedelta(days=days_ahead_start), now + timedelta(days=days_ahead_end))


def _seed_points_estimate(conn, *, project_id, workspace_id, value="5"):
    now = datetime.now(timezone.utc)
    est_id, ep_id = str(uuid.uuid4()), str(uuid.uuid4())
    with conn.cursor() as cur:
        cur.execute(
            "insert into estimates (id, name, description, project_id,"
            " workspace_id, type, last_used, created_at, updated_at)"
            " values (%s,%s,'',%s,%s,'points',false,%s,%s)",
            (est_id, f"Points {est_id[:8]}", project_id, workspace_id, now, now),
        )
        cur.execute(
            "insert into estimate_points (id, key, description, value,"
            " estimate_id, project_id, workspace_id, created_at, updated_at)"
            " values (%s,1,'',%s,%s,%s,%s,%s,%s)",
            (ep_id, value, est_id, project_id, workspace_id, now, now),
        )
        cur.execute("update projects set estimate_id=%s where id=%s", (est_id, project_id))
    return ep_id


def _seed_label(conn, *, project_id, workspace_id, name):
    label_id = str(uuid.uuid4())
    now = datetime.now(timezone.utc)
    with conn.cursor() as cur:
        cur.execute(
            "insert into labels (id, name, description, color, sort_order,"
            " project_id, workspace_id, created_at, updated_at)"
            " values (%s,%s,'','#4A90D9',65535,%s,%s,%s,%s)",
            (label_id, name, project_id, workspace_id, now, now),
        )
    return label_id


def _seed_issue(conn, *, workspace_id, project_id, state_id, created_by_id, name, sequence_id, estimate_point_id=None):
    issue_id = str(uuid.uuid4())
    now = datetime.now(timezone.utc)
    with conn.cursor() as cur:
        cur.execute(
            "insert into issues (id, name, description_json, priority,"
            " sequence_id, project_id, workspace_id, state_id, description_html,"
            " sort_order, is_draft, git_work_branch, workpad, complexity_score,"
            " estimate_point_id, created_at, updated_at, created_by_id)"
            " values (%s,%s,'{}','high',%s,%s,%s,%s,'<p></p>',65535,false,'','',0,"
            " %s,%s,%s,%s)",
            (issue_id, name, sequence_id, project_id, workspace_id, state_id, estimate_point_id, now, now, created_by_id),
        )
    return issue_id


def _create_dated_cycle(conn, *, workspace_id, project_id, owner_id, name, start_date, end_date):
    cycle_id = str(uuid.uuid4())
    now = datetime.now(timezone.utc)
    with conn.cursor() as cur:
        cur.execute(
            "insert into cycles (id, name, description, start_date, end_date,"
            " owned_by_id, project_id, workspace_id, view_props, sort_order,"
            " external_source, external_id, progress_snapshot, archived_at,"
            " logo_props, timezone, version, created_by_id, created_at, updated_at)"
            " values (%s,%s,'',%s,%s,%s,%s,%s,'{}',65535,null,null,'{}',null,'{}',"
            " 'UTC',1,%s,%s,%s)",
            (cycle_id, name, start_date, end_date, owner_id, project_id, workspace_id, owner_id, now, now),
        )
    return cycle_id


def _add_to_cycle(conn, *, cycle_id, issue_id, project_id, workspace_id):
    now = datetime.now(timezone.utc)
    with conn.cursor() as cur:
        cur.execute(
            "insert into cycle_issues (id, cycle_id, issue_id, project_id,"
            " workspace_id, created_at, updated_at)"
            " values (%s,%s,%s,%s,%s,%s,%s)",
            (str(uuid.uuid4()), cycle_id, issue_id, project_id, workspace_id, now, now),
        )


def _assign(conn, *, issue_id, assignee_id, project_id, workspace_id):
    now = datetime.now(timezone.utc)
    with conn.cursor() as cur:
        cur.execute(
            "insert into issue_assignees (id, issue_id, assignee_id, project_id,"
            " workspace_id, created_at, updated_at)"
            " values (%s,%s,%s,%s,%s,%s,%s)",
            (str(uuid.uuid4()), issue_id, assignee_id, project_id, workspace_id, now, now),
        )


def _label_issue(conn, *, issue_id, label_id, project_id, workspace_id):
    now = datetime.now(timezone.utc)
    with conn.cursor() as cur:
        cur.execute(
            "insert into issue_labels (id, issue_id, label_id, project_id,"
            " workspace_id, created_at, updated_at)"
            " values (%s,%s,%s,%s,%s,%s,%s)",
            (str(uuid.uuid4()), issue_id, label_id, project_id, workspace_id, now, now),
        )


@pytest.fixture()
def analytics_tenant(make_tenant):
    """Tenant whose side rows are swept (cycle_issues/labels/estimates are
    not covered by Seed.cleanup and would block the tracked deletes)."""
    tenant = make_tenant()
    yield tenant
    ws = tenant["workspace"]["id"]
    with tenant["conn"].cursor() as cur:
        cur.execute("update projects set estimate_id = null where workspace_id = %s", (ws,))
        cur.execute("delete from issue_labels where workspace_id = %s", (ws,))
        cur.execute("delete from issue_assignees where workspace_id = %s", (ws,))
        cur.execute("delete from cycle_issues where workspace_id = %s", (ws,))
        cur.execute("delete from issues where workspace_id = %s", (ws,))
        cur.execute("delete from labels where workspace_id = %s", (ws,))
        cur.execute("delete from estimate_points where workspace_id = %s", (ws,))
        cur.execute("delete from estimates where workspace_id = %s", (ws,))


def _bot_user(tenant, name="Bot"):
    bot = tenant["seed"].user(email=f"{name.lower()}-{uuid.uuid4().hex[:6]}@contract.test")
    with tenant["conn"].cursor() as cur:
        cur.execute("update users set display_name=%s where id=%s", (name, bot["id"]))
    return bot["id"]


def _build_live_pair(tenant, *, with_estimates=False):
    """Two issues assigned+labelled to Bot/Label plus one unassigned issue.

    Returns dict with cycle_id, bot_id, label_id and issue ids.
    All issues are pending (tenant state, no completed_at).
    """
    conn = tenant["conn"]
    ws = tenant["workspace"]["id"]
    proj = tenant["project"]["id"]
    state = tenant["state"]["id"]
    owner = tenant["user"]["id"]
    bot = _bot_user(tenant, "Bot")
    label = _seed_label(conn, project_id=proj, workspace_id=ws, name="Keep")
    ep = _seed_points_estimate(conn, project_id=proj, workspace_id=ws, value="5") if with_estimates else None
    start, end = _past_range()
    cycle_id = _create_dated_cycle(conn, workspace_id=ws, project_id=proj, owner_id=owner, name="DIFF19 cycle", start_date=start, end_date=end)
    tenant["seed"].track("cycles", "id", cycle_id)
    i1 = _seed_issue(conn, workspace_id=ws, project_id=proj, state_id=state, created_by_id=owner, name="I1", sequence_id=9201, estimate_point_id=ep)
    i2 = _seed_issue(conn, workspace_id=ws, project_id=proj, state_id=state, created_by_id=owner, name="I2", sequence_id=9202, estimate_point_id=ep)
    i3 = _seed_issue(conn, workspace_id=ws, project_id=proj, state_id=state, created_by_id=owner, name="I3", sequence_id=9203)
    for iid in (i1, i2, i3):
        _add_to_cycle(conn, cycle_id=cycle_id, issue_id=iid, project_id=proj, workspace_id=ws)
    _assign(conn, issue_id=i1, assignee_id=bot, project_id=proj, workspace_id=ws)
    _assign(conn, issue_id=i2, assignee_id=bot, project_id=proj, workspace_id=ws)
    _label_issue(conn, issue_id=i1, label_id=label, project_id=proj, workspace_id=ws)
    _label_issue(conn, issue_id=i2, label_id=label, project_id=proj, workspace_id=ws)
    return {"cycle_id": cycle_id, "bot": bot, "label": label, "i1": i1, "i2": i2, "i3": i3, "ep": ep}


def _soft_delete(tenant, ctx):
    conn = tenant["conn"]
    with conn.cursor() as cur:
        cur.execute("update issue_assignees set deleted_at=now() where issue_id=%s and assignee_id=%s", (ctx["i1"], ctx["bot"]))
        cur.execute("update issue_labels set deleted_at=now() where issue_id=%s and label_id=%s", (ctx["i1"], ctx["label"]))


def test_analytics_issues_excludes_soft_deleted_bridges(analytics_tenant):
    """Analytics ?type=issues: Bot/Keep count live only on Rust (1), over-count on Django (2)."""
    tenant = analytics_tenant
    ctx = _build_live_pair(tenant, with_estimates=False)
    _soft_delete(tenant, ctx)
    urls = cycle_urls(tenant)
    r = tenant["client"].get(urls["analytics"](ctx["cycle_id"]) + "?type=issues")
    assert r.status_code == 200, r.text[:600]
    body = r.json()

    assignees = {row["assignee_id"]: row for row in body["assignees"]}
    assert ctx["bot"] in assignees, body["assignees"]
    # Django keeps the soft-deleted I1 (2), Rust drops it (1). Completed stays 0, pending follows total.
    assert assignees[ctx["bot"]]["total_issues"] == expect("DIFF-21", django=2, rust=1), body["assignees"]
    assert assignees[ctx["bot"]]["completed_issues"] == 0
    assert assignees[ctx["bot"]]["pending_issues"] == expect("DIFF-21", django=2, rust=1)

    # The NULL (no-assignee) row is Count("assignee_id") -> 0 on both arms.
    null_rows = [row for row in body["assignees"] if row["assignee_id"] is None]
    assert len(null_rows) == 1, body["assignees"]
    assert null_rows[0]["total_issues"] == 0

    labels = {row["label_id"]: row for row in body["labels"]}
    assert ctx["label"] in labels, body["labels"]
    assert labels[ctx["label"]]["total_issues"] == expect("DIFF-21", django=2, rust=1), body["labels"]
    assert labels[ctx["label"]]["pending_issues"] == expect("DIFF-21", django=2, rust=1)
    null_labels = [row for row in body["labels"] if row["label_id"] is None]
    assert len(null_labels) == 1, body["labels"]
    assert null_labels[0]["total_issues"] == 0


def test_analytics_points_excludes_soft_deleted_bridges(analytics_tenant):
    """Analytics ?type=points: Bot/Keep sums drop the removed estimate on Rust."""
    tenant = analytics_tenant
    ctx = _build_live_pair(tenant, with_estimates=True)
    _soft_delete(tenant, ctx)
    urls = cycle_urls(tenant)
    r = tenant["client"].get(urls["analytics"](ctx["cycle_id"]) + "?type=points")
    assert r.status_code == 200, r.text[:600]
    body = r.json()

    assignees = {row["assignee_id"]: row for row in body["assignees"]}
    assert ctx["bot"] in assignees, body["assignees"]
    # Two 5-point issues: Django sums 10, Rust sums 5 after I1 drops out.
    assert assignees[ctx["bot"]]["total_estimates"] == expect("DIFF-21", django=10.0, rust=5.0), body["assignees"]
    assert assignees[ctx["bot"]]["pending_estimates"] == expect("DIFF-21", django=10.0, rust=5.0)
    assert assignees[ctx["bot"]]["completed_estimates"] is None

    # The removed I1 (5 points) falls to the NULL group on Rust; Django keeps it under Bot so NULL stays None.
    null_rows = [row for row in body["assignees"] if row["assignee_id"] is None]
    assert len(null_rows) == 1, body["assignees"]
    assert null_rows[0]["total_estimates"] == expect("DIFF-21", django=None, rust=5.0), null_rows[0]
    assert null_rows[0]["pending_estimates"] == expect("DIFF-21", django=None, rust=5.0)

    labels = {row["label_id"]: row for row in body["labels"]}
    assert ctx["label"] in labels, body["labels"]
    assert labels[ctx["label"]]["total_estimates"] == expect("DIFF-21", django=10.0, rust=5.0), body["labels"]
    null_labels = [row for row in body["labels"] if row["label_id"] is None]
    assert len(null_labels) == 1, body["labels"]
    assert null_labels[0]["total_estimates"] == expect("DIFF-21", django=None, rust=5.0), null_labels[0]


def test_archived_distributions_exclude_soft_deleted_bridges(analytics_tenant):
    """Archived detail distributions use Count("id"): NULL group shows unassigned count (1 Django, 2 Rust)."""
    tenant = analytics_tenant
    ctx = _build_live_pair(tenant, with_estimates=True)
    _soft_delete(tenant, ctx)
    urls = cycle_urls(tenant)
    r = tenant["client"].post(urls["archive"](ctx["cycle_id"]))
    assert r.status_code == 200, r.text[:600]
    assert r.json()["archived_at"] is not None

    r = tenant["client"].get(urls["archived_detail"](ctx["cycle_id"]))
    assert r.status_code == 200, r.text[:600]
    detail = r.json()

    # Issue-count distribution
    assignees = {row["assignee_id"]: row for row in detail["distribution"]["assignees"]}
    assert ctx["bot"] in assignees, detail["distribution"]["assignees"]
    assert assignees[ctx["bot"]]["total_issues"] == expect("DIFF-21", django=2, rust=1), detail["distribution"]["assignees"]
    null_assignees = [row for row in detail["distribution"]["assignees"] if row["assignee_id"] is None]
    assert len(null_assignees) == 1, detail["distribution"]["assignees"]
    assert null_assignees[0]["total_issues"] == expect("DIFF-21", django=1, rust=2), null_assignees[0]

    labels = {row["label_id"]: row for row in detail["distribution"]["labels"]}
    assert ctx["label"] in labels, detail["distribution"]["labels"]
    assert labels[ctx["label"]]["total_issues"] == expect("DIFF-21", django=2, rust=1)
    null_labels = [row for row in detail["distribution"]["labels"] if row["label_id"] is None]
    assert len(null_labels) == 1
    assert null_labels[0]["total_issues"] == expect("DIFF-21", django=1, rust=2), null_labels[0]

    # Estimate distribution
    est_assignees = {row["assignee_id"]: row for row in detail["estimate_distribution"]["assignees"]}
    assert ctx["bot"] in est_assignees, detail["estimate_distribution"]["assignees"]
    assert est_assignees[ctx["bot"]]["total_estimates"] == expect("DIFF-21", django=10.0, rust=5.0)
    null_est = [row for row in detail["estimate_distribution"]["assignees"] if row["assignee_id"] is None]
    assert len(null_est) == 1
    assert null_est[0]["total_estimates"] == expect("DIFF-21", django=None, rust=5.0), null_est[0]

    est_labels = {row["label_id"]: row for row in detail["estimate_distribution"]["labels"]}
    assert ctx["label"] in est_labels
    assert est_labels[ctx["label"]]["total_estimates"] == expect("DIFF-21", django=10.0, rust=5.0)
    null_est_labels = [row for row in detail["estimate_distribution"]["labels"] if row["label_id"] is None]
    assert len(null_est_labels) == 1
    assert null_est_labels[0]["total_estimates"] == expect("DIFF-21", django=None, rust=5.0)


def test_transfer_snapshot_excludes_soft_deleted_bridges(analytics_tenant):
    """Transfer progress_snapshot distributions drop the soft-deleted bridge on Rust."""
    tenant = analytics_tenant
    ctx = _build_live_pair(tenant, with_estimates=True)
    _soft_delete(tenant, ctx)

    conn = tenant["conn"]
    ws = tenant["workspace"]["id"]
    proj = tenant["project"]["id"]
    owner = tenant["user"]["id"]
    start, end = _future_range()
    dst = _create_dated_cycle(conn, workspace_id=ws, project_id=proj, owner_id=owner, name="Dest", start_date=start, end_date=end)
    tenant["seed"].track("cycles", "id", dst)

    urls = cycle_urls(tenant)
    r = tenant["client"].post(urls["transfer"](ctx["cycle_id"]), json={"new_cycle_id": dst})
    assert r.status_code == 200, r.text[:600]
    assert r.json() == {"message": "Success"}

    with conn.cursor() as cur:
        cur.execute("select progress_snapshot from cycles where id=%s", (ctx["cycle_id"],))
        snapshot = cur.fetchone()[0]
    assert snapshot, "transfer must store a snapshot"

    dist_assignees = {row["assignee_id"]: row for row in snapshot["distribution"]["assignees"]}
    assert ctx["bot"] in dist_assignees, snapshot["distribution"]["assignees"]
    assert dist_assignees[ctx["bot"]]["total_issues"] == expect("DIFF-21", django=2, rust=1), snapshot["distribution"]["assignees"]
    null_dist = [row for row in snapshot["distribution"]["assignees"] if row["assignee_id"] is None]
    assert len(null_dist) == 1
    assert null_dist[0]["total_issues"] == expect("DIFF-21", django=1, rust=2), null_dist[0]

    dist_labels = {row["label_id"]: row for row in snapshot["distribution"]["labels"]}
    assert ctx["label"] in dist_labels
    assert dist_labels[ctx["label"]]["total_issues"] == expect("DIFF-21", django=2, rust=1)
    null_dist_labels = [row for row in snapshot["distribution"]["labels"] if row["label_id"] is None]
    assert len(null_dist_labels) == 1
    assert null_dist_labels[0]["total_issues"] == expect("DIFF-21", django=1, rust=2)

    est_assignees = {row["assignee_id"]: row for row in snapshot["estimate_distribution"]["assignees"]}
    assert ctx["bot"] in est_assignees
    assert est_assignees[ctx["bot"]]["total_estimates"] == expect("DIFF-21", django=10.0, rust=5.0)
    null_est = [row for row in snapshot["estimate_distribution"]["assignees"] if row["assignee_id"] is None]
    assert len(null_est) == 1
    assert null_est[0]["total_estimates"] == expect("DIFF-21", django=None, rust=5.0)

    # The stored snapshot is also served via analytics (snapshot branch).
    r = tenant["client"].get(urls["analytics"](ctx["cycle_id"]) + "?type=issues")
    assert r.status_code == 200, r.text[:600]
    served = {row["assignee_id"]: row for row in r.json()["assignees"]}
    assert served[ctx["bot"]]["total_issues"] == expect("DIFF-21", django=2, rust=1), r.json()["assignees"]
