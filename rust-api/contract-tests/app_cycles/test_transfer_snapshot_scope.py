"""PIDASHCONV-1183: the transfer ``progress_snapshot`` (and the analytics /
progress endpoints that serve it) must exclude triage-state, archived and
draft issues, exactly the way ``Issue.issue_objects`` does in Django.

Django builds every per-issue base queryset behind the snapshot through
``IssueManager`` (``db/models/issue.py:95-104``), which drops soft-deleted,
triage-state, archived, project-archived and draft issues. The five transfer
queries in ``handlers_misc.rs`` filtered only ``i.deleted_at IS NULL`` and so
counted the excluded shapes: a triage issue lands in the distributions /
burndown, an archived- or draft-only assignee or label adds a zero/NULL group
row Django omits, and archived/draft completions land in the chart.

The suite seeds one dated source cycle holding each excluded shape plus a
normal completed/open pair, POSTs ``transfer-issues`` and pins the stored
snapshot, the served analytics and the progress counts.
"""

import uuid
from datetime import datetime, timedelta, timezone

import pytest

from .conftest import cycle_urls

pytestmark = pytest.mark.contract


def _insert(seed, table, **cols):
    return seed._insert(table, **cols)


class _Scope:
    """Seeded scenario plus the values Django is expected to store."""

    def __init__(self, tenant, ts, start, end, done_at):
        self.tenant = tenant
        self.ts = ts
        self.start = start
        self.end = end
        self.done_at = done_at
        self.src = None
        self.dst = None
        self.assignee_n = None
        self.label_n = None
        self.conn = tenant["conn"]


def _seed_scope(tenant) -> _Scope:
    seed = tenant["seed"]
    conn = tenant["conn"]
    wid = tenant["workspace"]["id"]
    pid = tenant["project"]["id"]
    uid = tenant["user"]["id"]
    ts = datetime.now(timezone.utc)
    start = ts - timedelta(days=30)
    end = ts - timedelta(days=10)
    done_at = ts - timedelta(days=22)

    s_triage = seed.state(pid, wid, name="Triage", group="triage")
    s_started = seed.state(pid, wid, name="Started", group="started")
    s_completed = seed.state(pid, wid, name="Completed", group="completed")

    # A points estimate linked to the project, so `project_uses_points` is
    # true and the estimate_distribution keys are populated.
    est = _insert(
        seed, "estimates", name=f"points-{uuid.uuid4().hex[:8]}", description="",
        project_id=pid, workspace_id=wid, type="points", last_used=False,
        created_at=ts, updated_at=ts,
    )
    with conn.cursor() as cur:
        cur.execute("UPDATE projects SET estimate_id=%s WHERE id=%s", (est, pid))

    counter = {"n": 5000}

    def seq():
        counter["n"] += 1
        return counter["n"]

    def point(value, key):
        return _insert(
            seed, "estimate_points", key=key, description="", value=value,
            estimate_id=est, project_id=pid, workspace_id=wid,
            created_at=ts, updated_at=ts,
        )

    def named_user(name):
        u = seed.user(email=f"{name.lower()}-{uuid.uuid4().hex[:6]}@contract.test")
        with conn.cursor() as cur:
            cur.execute(
                "UPDATE users SET display_name=%s WHERE id=%s", (name, u["id"])
            )
        return u["id"]

    def label_row(name):
        return _insert(
            seed, "labels", name=name, description="", color="#123456",
            project_id=pid, workspace_id=wid, sort_order=65535,
            created_at=ts, updated_at=ts,
        )

    def issue(name, state_id, *, archived_at=None, is_draft=False,
              completed_at=None, estimate_point_id=None):
        return _insert(
            seed, "issues", name=name, description_json="{}",
            description_html="<p></p>", priority="none", sequence_id=seq(),
            state_id=state_id, project_id=pid, workspace_id=wid,
            sort_order=65535, is_draft=is_draft, git_work_branch="", workpad="",
            complexity_score=0, created_at=ts, updated_at=ts,
            archived_at=archived_at, completed_at=completed_at,
            estimate_point_id=estimate_point_id, created_by_id=uid,
        )

    def bridge(issue_id, cycle_id):
        _insert(
            seed, "cycle_issues", cycle_id=cycle_id, issue_id=issue_id,
            project_id=pid, workspace_id=wid, created_at=ts, updated_at=ts,
            created_by_id=uid,
        )

    def assign(issue_id, assignee_id):
        _insert(
            seed, "issue_assignees", assignee_id=assignee_id, issue_id=issue_id,
            project_id=pid, workspace_id=wid, created_at=ts, updated_at=ts,
        )

    def label(issue_id, label_id):
        _insert(
            seed, "issue_labels", label_id=label_id, issue_id=issue_id,
            project_id=pid, workspace_id=wid, created_at=ts, updated_at=ts,
        )

    def cycle(name, cstart, cend):
        return _insert(
            seed, "cycles", name=name, description="", start_date=cstart,
            end_date=cend, owned_by_id=uid, project_id=pid, workspace_id=wid,
            view_props="{}", sort_order=65535, logo_props="{}", timezone="UTC",
            version=1, created_by_id=uid, created_at=ts, updated_at=ts,
            progress_snapshot="{}",
        )

    scope = _Scope(tenant, ts, start, end, done_at)
    scope.src = cycle("Source", start, end)
    scope.dst = cycle("Dest", ts, ts + timedelta(days=20))

    u_triage = named_user("Triage")
    u_arch = named_user("Archived")
    u_draft = named_user("Draft")
    u_norm = named_user("Norm")
    scope.assignee_n = u_norm
    l_triage = label_row("L-Triage")
    l_arch = label_row("L-Archived")
    l_draft = label_row("L-Draft")
    l_norm = label_row("L-Norm")
    scope.label_n = l_norm

    excluded = [
        # (issue, state, assignee, label, completed_at, kwargs extras)
        (issue("triage", s_triage, completed_at=ts - timedelta(days=25),
               estimate_point_id=point("1", 1)), u_triage, l_triage),
        (issue("archived", s_completed, archived_at=ts - timedelta(days=20),
               completed_at=ts - timedelta(days=24),
               estimate_point_id=point("1", 1)), u_arch, l_arch),
        (issue("draft", s_completed, is_draft=True,
               completed_at=ts - timedelta(days=23),
               estimate_point_id=point("1", 1)), u_draft, l_draft),
    ]
    for iid, au, lid in excluded:
        bridge(iid, scope.src)
        assign(iid, au)
        label(iid, lid)

    done = issue("done", s_completed, completed_at=done_at,
                 estimate_point_id=point("2", 2))
    open_ = issue("open", s_started, estimate_point_id=point("3", 3))
    for iid in (done, open_):
        bridge(iid, scope.src)
        assign(iid, u_norm)
        label(iid, l_norm)

    return scope


def _expected_distribution(scope):
    return {
        "labels": [
            {
                "label_name": "L-Norm",
                "color": "#123456",
                "label_id": scope.label_n,
                "total_issues": 2,
                "completed_issues": 1,
                "pending_issues": 1,
            }
        ],
        "assignees": [
            {
                "display_name": "Norm",
                "assignee_id": scope.assignee_n,
                "avatar_url": "",
                "total_issues": 2,
                "completed_issues": 1,
                "pending_issues": 1,
            }
        ],
        "completion_chart": _chart(scope, 3, 2),
    }


def _expected_estimate_distribution(scope):
    return {
        "labels": [
            {
                "label_name": "L-Norm",
                "color": "#123456",
                "label_id": scope.label_n,
                "total_estimates": 5.0,
                "completed_estimates": 2.0,
                "pending_estimates": 3.0,
            }
        ],
        "assignees": [
            {
                "display_name": "Norm",
                "assignee_id": scope.assignee_n,
                "avatar_url": "",
                "total_estimates": 5.0,
                "completed_estimates": 2.0,
                "pending_estimates": 3.0,
            }
        ],
        "completion_chart": _chart(scope, 5.0, 3.0),
    }


def _chart(scope, before, after):
    """Burndown over the source range: `before` up to the completed day,
    `after` from then on. Only the normal issue counts (issue_objects)."""
    completion = scope.done_at.date()
    day = scope.start.date()
    last = scope.end.date()
    out = {}
    while day <= last:
        out[day.isoformat()] = before if day < completion else after
        day += timedelta(days=1)
    return out


@pytest.fixture()
def transfer_scope(make_tenant):
    tenant = make_tenant()
    scope = _seed_scope(tenant)
    yield scope
    # issue_labels / issue_assignees point at issues; delete them before
    # the shared Seed.cleanup() workspace sweep tries to drop the issues.
    with tenant["conn"].cursor() as cur:
        cur.execute(
            "UPDATE projects SET estimate_id=NULL WHERE id=%s",
            (tenant["project"]["id"],),
        )
        cur.execute(
            "DELETE FROM issue_labels WHERE workspace_id=%s",
            (tenant["workspace"]["id"],),
        )
        cur.execute(
            "DELETE FROM issue_assignees WHERE workspace_id=%s",
            (tenant["workspace"]["id"],),
        )
        cur.execute(
            "DELETE FROM cycle_issues WHERE workspace_id=%s",
            (tenant["workspace"]["id"],),
        )


def _run_transfer(scope):
    urls = cycle_urls(scope.tenant)
    client = scope.tenant["client"]
    r = client.post(
        urls["transfer"](scope.src), json={"new_cycle_id": scope.dst}
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json() == {"message": "Success"}
    return urls


def _stored_snapshot(scope):
    with scope.conn.cursor() as cur:
        cur.execute(
            "SELECT progress_snapshot FROM cycles WHERE id=%s", (scope.src,)
        )
        row = cur.fetchone()
    assert row is not None and row[0], "transfer must store a snapshot"
    return row[0]


def test_transfer_snapshot_excludes_triage_archived_draft(transfer_scope):
    scope = transfer_scope
    _run_transfer(scope)
    snapshot = _stored_snapshot(scope)

    assert snapshot["total_issues"] == 3
    assert snapshot["completed_issues"] == 1
    assert snapshot["cancelled_issues"] == 0
    assert snapshot["started_issues"] == 1
    assert snapshot["unstarted_issues"] == 0
    assert snapshot["backlog_issues"] == 0

    assert snapshot["distribution"] == _expected_distribution(scope)
    assert snapshot["estimate_distribution"] == _expected_estimate_distribution(
        scope
    )


def test_analytics_serves_snapshot_scoped_distribution(transfer_scope):
    scope = transfer_scope
    urls = _run_transfer(scope)
    expected = _expected_distribution(scope)

    for analytic_type in ("issues", "points"):
        r = scope.tenant["client"].get(
            urls["analytics"](scope.src), params={"type": analytic_type}
        )
        assert r.status_code == 200, r.text[:500]
        assert r.json() == expected, analytic_type


def test_progress_serves_snapshot_scoped_counts(transfer_scope):
    scope = transfer_scope
    urls = _run_transfer(scope)

    r = scope.tenant["client"].get(urls["progress"](scope.src))
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    assert body["total_issues"] == 3
    assert body["completed_issues"] == 1
    assert body["started_issues"] == 1
    assert body["unstarted_issues"] == 0
    assert body["cancelled_issues"] == 0
    assert body["backlog_issues"] == 0
