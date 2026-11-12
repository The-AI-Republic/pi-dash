"""Points distributions keep unestimated cycle issues (PIDASHCONV-1185).

Django groups the full ``Issue.issue_objects`` base for a cycle and
annotates ``Sum(Cast("estimate_point__value", FloatField()))`` with no
``estimate_point`` filter (``app/views/cycle/base.py:843-931`` and
``app/views/cycle/archive.py:367-454``). ``Issue.estimate_point`` is a
nullable FK, so the ORM emits a LEFT OUTER JOIN: an issue without an
estimate point still forms its assignee/label group with NULL sums, and the
NULL-unassigned row survives when only unestimated issues are unassigned.
The Rust port INNER-JOINed ``estimate_points`` and dropped those rows before
grouping. These tests pin the Django row set for ``analytics ?type=points``
and for the archived-cycle ``estimate_distribution``.
"""

import uuid
from datetime import datetime, timedelta, timezone

import pytest

from .conftest import cycle_urls

pytestmark = pytest.mark.contract


def _past_range(days_ago_start=40, days_ago_end=5):
    now = datetime.now(timezone.utc).date()
    return (
        (now - timedelta(days=days_ago_start)).isoformat(),
        (now - timedelta(days=days_ago_end)).isoformat(),
    )


def _seed_points_estimate(conn, *, project_id, workspace_id, value="5"):
    """A ``points`` estimate with one point, linked to the project."""
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
        cur.execute(
            "update projects set estimate_id=%s where id=%s", (est_id, project_id)
        )
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


def _seed_issue(
    conn, *, workspace_id, project_id, state_id, created_by_id, name,
    sequence_id, estimate_point_id=None,
):
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
            (issue_id, name, sequence_id, project_id, workspace_id, state_id,
             estimate_point_id, now, now, created_by_id),
        )
    return issue_id


def _create_dated_cycle(
    conn, *, workspace_id, project_id, owner_id, name, start_date, end_date,
):
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
            (cycle_id, name, start_date, end_date, owner_id, project_id,
             workspace_id, owner_id, now, now),
        )
    return cycle_id


def _add_to_cycle(conn, *, cycle_id, issue_id, project_id, workspace_id):
    now = datetime.now(timezone.utc)
    with conn.cursor() as cur:
        cur.execute(
            "insert into cycle_issues (id, cycle_id, issue_id, project_id,"
            " workspace_id, created_at, updated_at)"
            " values (%s,%s,%s,%s,%s,%s,%s)",
            (str(uuid.uuid4()), cycle_id, issue_id, project_id, workspace_id,
             now, now),
        )


def _assign(conn, *, issue_id, assignee_id, project_id, workspace_id):
    now = datetime.now(timezone.utc)
    with conn.cursor() as cur:
        cur.execute(
            "insert into issue_assignees (id, issue_id, assignee_id, project_id,"
            " workspace_id, created_at, updated_at)"
            " values (%s,%s,%s,%s,%s,%s,%s)",
            (str(uuid.uuid4()), issue_id, assignee_id, project_id, workspace_id,
             now, now),
        )


def _label_issue(conn, *, issue_id, label_id, project_id, workspace_id):
    now = datetime.now(timezone.utc)
    with conn.cursor() as cur:
        cur.execute(
            "insert into issue_labels (id, issue_id, label_id, project_id,"
            " workspace_id, created_at, updated_at)"
            " values (%s,%s,%s,%s,%s,%s,%s)",
            (str(uuid.uuid4()), issue_id, label_id, project_id, workspace_id,
             now, now),
        )


@pytest.fixture()
def points_tenant(make_tenant):
    """A tenant whose own side rows are swept before ``make_tenant`` teardown.

    ``Seed.cleanup`` does not delete ``cycle_issues`` / ``issue_assignees`` /
    ``issue_labels`` / ``labels`` / ``estimates`` / ``estimate_points`` (all
    FKs are NO ACTION and the connection autocommits), so clear ours by
    workspace first — otherwise the tracked project delete trips a live FK.
    """
    tenant = make_tenant()
    yield tenant
    ws = tenant["workspace"]["id"]
    with tenant["conn"].cursor() as cur:
        cur.execute(
            "update projects set estimate_id = null where workspace_id = %s", (ws,)
        )
        cur.execute("delete from issue_labels where workspace_id = %s", (ws,))
        cur.execute("delete from issue_assignees where workspace_id = %s", (ws,))
        cur.execute("delete from cycle_issues where workspace_id = %s", (ws,))
        cur.execute("delete from issues where workspace_id = %s", (ws,))
        cur.execute("delete from labels where workspace_id = %s", (ws,))
        cur.execute("delete from estimate_points where workspace_id = %s", (ws,))
        cur.execute("delete from estimates where workspace_id = %s", (ws,))


def _build(points_tenant, *, est_assigned=True, non_assigned=True,
           label_other=True):
    """One estimated + one unestimated issue on a dated cycle.

    Returns a dict with the ids the assertions key on.
    """
    tenant = points_tenant
    conn = tenant["conn"]
    ws = tenant["workspace"]["id"]
    proj = tenant["project"]["id"]
    state = tenant["state"]["id"]
    owner = tenant["user"]["id"]

    ep_id = _seed_points_estimate(conn, project_id=proj, workspace_id=ws, value="5")
    user_a = tenant["user"]["id"]
    user_b = tenant["seed"].user()["id"]
    label_a = _seed_label(conn, project_id=proj, workspace_id=ws, name="cm-used-a")
    label_b = _seed_label(conn, project_id=proj, workspace_id=ws, name="cm-unused-b")

    start, end = _past_range()
    cycle_id = _create_dated_cycle(
        conn, workspace_id=ws, project_id=proj, owner_id=owner,
        name="Points distribution", start_date=start, end_date=end,
    )
    tenant["seed"].track("cycles", "id", cycle_id)

    est_issue = _seed_issue(
        conn, workspace_id=ws, project_id=proj, state_id=state,
        created_by_id=owner, name="Estimated", sequence_id=9101,
        estimate_point_id=ep_id,
    )
    non_issue = _seed_issue(
        conn, workspace_id=ws, project_id=proj, state_id=state,
        created_by_id=owner, name="Unestimated", sequence_id=9102,
    )
    _add_to_cycle(conn, cycle_id=cycle_id, issue_id=est_issue,
                  project_id=proj, workspace_id=ws)
    _add_to_cycle(conn, cycle_id=cycle_id, issue_id=non_issue,
                  project_id=proj, workspace_id=ws)
    if est_assigned:
        _assign(conn, issue_id=est_issue, assignee_id=user_a,
                project_id=proj, workspace_id=ws)
    if non_assigned:
        _assign(conn, issue_id=non_issue, assignee_id=user_b,
                project_id=proj, workspace_id=ws)
    if label_other:
        _label_issue(conn, issue_id=est_issue, label_id=label_a,
                     project_id=proj, workspace_id=ws)
        _label_issue(conn, issue_id=non_issue, label_id=label_b,
                     project_id=proj, workspace_id=ws)
    return {
        "cycle_id": cycle_id, "user_a": user_a, "user_b": user_b,
        "label_a": label_a, "label_b": label_b,
    }


def _empty_sums(row):
    return (
        row["total_estimates"] is None
        and row["completed_estimates"] is None
        and row["pending_estimates"] is None
    )


def test_analytics_points_keeps_unestimated_contributors(points_tenant):
    ctx = _build(points_tenant)
    urls = cycle_urls(points_tenant)
    r = points_tenant["client"].get(
        urls["analytics"](ctx["cycle_id"]) + "?type=points"
    )
    assert r.status_code == 200, r.text[:500]
    body = r.json()

    assignees = {row["assignee_id"]: row for row in body["assignees"]}
    assert ctx["user_b"] in assignees, body["assignees"]
    assert _empty_sums(assignees[ctx["user_b"]]), assignees[ctx["user_b"]]
    assert assignees[ctx["user_a"]]["total_estimates"] == 5.0

    labels = {row["label_id"]: row for row in body["labels"]}
    assert ctx["label_b"] in labels, body["labels"]
    assert _empty_sums(labels[ctx["label_b"]]), labels[ctx["label_b"]]
    assert labels[ctx["label_a"]]["total_estimates"] == 5.0


def test_analytics_points_keeps_null_unassigned_row(points_tenant):
    # The unassigned row (assignee_id null) survives while its only cycle
    # issue is unestimated; the estimated issue carries the real sums.
    ctx = _build(points_tenant, est_assigned=True, non_assigned=False)
    urls = cycle_urls(points_tenant)
    r = points_tenant["client"].get(
        urls["analytics"](ctx["cycle_id"]) + "?type=points"
    )
    assert r.status_code == 200, r.text[:500]
    body = r.json()

    null_rows = [row for row in body["assignees"] if row["assignee_id"] is None]
    assert len(null_rows) == 1, body["assignees"]
    assert _empty_sums(null_rows[0]), null_rows[0]
    assert {row["assignee_id"] for row in body["assignees"]} == {
        None, ctx["user_a"]
    }


def test_archived_points_keeps_unestimated_contributors(points_tenant):
    ctx = _build(points_tenant)
    urls = cycle_urls(points_tenant)
    r = points_tenant["client"].post(urls["archive"](ctx["cycle_id"]))
    assert r.status_code == 200, r.text[:500]
    assert r.json()["archived_at"] is not None

    r = points_tenant["client"].get(urls["archived_detail"](ctx["cycle_id"]))
    assert r.status_code == 200, r.text[:500]
    dist = r.json()["estimate_distribution"]

    assignees = {row["assignee_id"]: row for row in dist["assignees"]}
    assert ctx["user_b"] in assignees, dist["assignees"]
    assert _empty_sums(assignees[ctx["user_b"]]), assignees[ctx["user_b"]]
    assert assignees[ctx["user_a"]]["total_estimates"] == 5.0

    labels = {row["label_id"]: row for row in dist["labels"]}
    assert ctx["label_b"] in labels, dist["labels"]
    assert _empty_sums(labels[ctx["label_b"]]), labels[ctx["label_b"]]
    assert labels[ctx["label_a"]]["total_estimates"] == 5.0
