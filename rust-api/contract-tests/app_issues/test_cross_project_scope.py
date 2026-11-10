"""Cross-project write scope (NEWPIDASH-170, DIFF-13).

The comment / issue-reaction / comment-reaction / subscribe /
issue-subscribers create paths gate on the caller's membership in the
*URL's* project and then act on a work item (or comment) loaded by
primary key only — never checking that it belongs to that project.
Any workspace member of any project can therefore write to a private
project's work item through a different project's path, and the
comment-activity task auto-subscribes them to its notification stream.

Old bug fix: the new system refuses the cross-project write with
404 {"error": "The required object does not exist."} (the existing
missing-work-item body on these routes) while Django pins its old
201. A missing work item answers the same 404 on Rust.

Ordering note: the explicit subscribe / issue-subscribers posts run
before the comment post on the Django arm because the comment's
auto-subscription row (created eagerly in tests) would otherwise
unique-conflict with them (Django answers 400 "The payload is not
valid" for a second subscription of the same user).

Seed: private project XB (network 0) + work item    XB1 in it, built by
SQL with fresh ids so nothing else in the session suite shares them;
member belongs to project A (``IS``) but not to XB. Setup comment C5 on
XB1 as admin (who is XB's only member) via XB's own path → 201 both.
"""

import uuid
from datetime import datetime, timezone

from _harness.diffs import expect, is_rust

NOW = datetime.now(timezone.utc)

FORBIDDEN = {"error": "You don't have the required permissions."}
NOT_FOUND = {"error": "The required object does not exist."}


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def _issue(ws, pid, iid):
    return f"{_base(ws, pid)}/issues/{iid}"


def _setup_private_world(db_conn, seed):
    """SQL-seed private project XB, its backlog state, work item XB1."""
    proj = str(uuid.uuid4())
    state = str(uuid.uuid4())
    issue = str(uuid.uuid4())
    identifier = "XB%s" % uuid.uuid4().hex[:6].upper()
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO projects (created_at, updated_at, id, name,"
            " description, network, identifier, workspace_id, cycle_view,"
            " module_view, issue_views_view, page_view, intake_view,"
            " archive_in, close_in, logo_props, is_time_tracking_enabled,"
            " is_issue_type_enabled, guest_view_all_features, timezone,"
            " members_can_edit_states, repo_url, base_branch,"
            " agent_default_interval_seconds, agent_default_max_ticks,"
            " agent_ticking_enabled, is_default,"
            " agent_review_default_interval_seconds, default_agent_executor,"
            " dedicated_runner_lost_action, agent_test_default_interval_seconds)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,"
            "%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, proj, "XB private", "", 0, identifier, seed["ws"],
             True, True, True, True, True, 0, 0, "{}", True, True, False,
             "UTC", True, "", "", 3600, 100, True, False, 10800, "muse",
             "fail_to_backlog", 10800),
        )
        cur.execute(
            "INSERT INTO project_members (created_at, updated_at, id, role,"
            " member_id, project_id, workspace_id, view_props, default_props,"
            " sort_order, preferences, is_active)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, str(uuid.uuid4()), 20, seed["admin"], proj,
             seed["ws"], "{}", "{}", 0.0, "{}", True),
        )
        cur.execute(
            "INSERT INTO states (created_at, updated_at, id, name,"
            " description, color, slug, project_id, workspace_id, sequence,"
            " \"group\", \"default\", is_triage)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, state, "XB Backlog", "", "#000000", "xb-backlog",
             proj, seed["ws"], 0.0, "backlog", True, False),
        )
        cur.execute(
            "INSERT INTO issues (created_at, updated_at, id, name,"
            " description_json, priority, sequence_id, created_by_id,"
            " project_id, state_id, workspace_id, description_html,"
            " sort_order, point, completed_at, is_draft, git_work_branch,"
            " workpad, complexity_score)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,"
            "%s,%s,%s)",
            (NOW, NOW, issue, "XB1 private", "{}", "medium", 99,
             seed["admin"], proj, state, seed["ws"], "", 0.0, None, None,
             False, "", "", 0),
        )
    return {"project": proj, "state": state, "issue": issue}


def _teardown_world(db_conn, world):
    """Delete everything the test built, FK order reversed, best effort."""
    issue = world["issue"]
    with db_conn.cursor() as cur:
        cur.execute(
            "DELETE FROM issue_subscribers WHERE issue_id = %s", (issue,))
        cur.execute(
            "DELETE FROM comment_reactions WHERE comment_id IN "
            "(SELECT id FROM issue_comments WHERE issue_id = %s)", (issue,))
        cur.execute(
            "DELETE FROM issue_reactions WHERE issue_id = %s", (issue,))
        cur.execute(
            "DELETE FROM issue_comments WHERE issue_id = %s", (issue,))
        # A comment's save also writes its `descriptions` twin row
        # (Django `IssueComment.save` order, mirrored by Rust). It is
        # referenced by the now-deleted comment, so it deletes after
        # it, scoped by the fresh project id (descriptions has no
        # issue column).
        cur.execute(
            "DELETE FROM descriptions WHERE project_id = %s",
            (world["project"],))
        cur.execute(
            "DELETE FROM issue_activities WHERE issue_id = %s", (issue,))
        cur.execute("DELETE FROM issues WHERE id = %s", (issue,))
        cur.execute(
            "DELETE FROM project_members WHERE project_id = %s",
            (world["project"],))
        cur.execute("DELETE FROM states WHERE id = %s", (world["state"],))
        cur.execute("DELETE FROM projects WHERE id = %s", (world["project"],))


def _subscriber_count(db_conn, issue_id, subscriber_id):
    """Live (non soft-deleted) subscription rows for the caller.

    The Django destroy/unsubscribe paths soft-delete, so only live
    rows are counted — a soft-deleted row is no subscription.
    """
    with db_conn.cursor() as cur:
        cur.execute(
            "SELECT count(*) FROM issue_subscribers"
            " WHERE issue_id = %s AND subscriber_id = %s"
            " AND deleted_at IS NULL",
            (issue_id, subscriber_id),
        )
        return cur.fetchone()[0]


def test_writes_via_foreign_project_path(clients, seed, db_conn):
    admin, member = clients["admin"], clients["member"]
    ws = seed["ws_slug"]
    pid_a = seed["project"]
    i2 = seed["issue2"]
    world = _setup_private_world(db_conn, seed)
    pid_b, xb1 = world["project"], world["issue"]
    created = []
    try:
        # Setup: admin (XB's only member) comments on XB1 via XB → 201 both.
        c5 = admin.post(
            f"{_issue(ws, pid_b, xb1)}/comments/",
            json={"comment_html": "<p>xb1 secret</p>",
                  "comment_stripped": "xb1 secret"},
        ).json()["id"]
        created.append(("c5", c5))

        # Control: member comments in own project A on I2 → 201 both.
        control = member.post(
            f"{_issue(ws, pid_a, i2)}/comments/",
            json={"comment_html": "<p>own project</p>",
                  "comment_stripped": "own project"},
        )
        assert control.status_code == 201
        assert member.delete(
            f"{_issue(ws, pid_a, i2)}/comments/{control.json()['id']}/"
        ).status_code == 204

        # Direct path as a non-member: 403 on both backends.
        resp = member.post(
            f"{_issue(ws, pid_b, xb1)}/comments/",
            json={"comment_html": "<p>direct</p>", "comment_stripped": "direct"},
        )
        assert resp.status_code == 403
        assert resp.json() == FORBIDDEN

        # Subscribe via A → DH for old 201 / 404 new.
        resp = member.post(f"{_issue(ws, pid_a, xb1)}/subscribe/")
        assert resp.status_code == expect("DIFF-13", django=201, rust=404)
        if is_rust():
            assert resp.json() == NOT_FOUND
        else:
            assert member.delete(
                f"{_issue(ws, pid_a, xb1)}/subscribe/").status_code == 204

        # Issue-subscribers create (for the member) via A → same split.
        resp = admin.post(
            f"{_issue(ws, pid_a, xb1)}/issue-subscribers/",
            json={"subscriber": seed["member"], "deleted_at": None},
        )
        assert resp.status_code == expect("DIFF-13", django=201, rust=404)
        if is_rust():
            assert resp.json() == NOT_FOUND
        else:
            assert admin.request(
                "DELETE",
                f"{_issue(ws, pid_a, xb1)}/issue-subscribers/"
                f"{seed['member']}/",
            ).status_code == 204

        # Issue reaction via A → 201 django / 404 rust.
        resp = member.post(
            f"{_issue(ws, pid_a, xb1)}/reactions/", json={"reaction": "🍊"})
        assert resp.status_code == expect("DIFF-13", django=201, rust=404)
        if is_rust():
            assert resp.json() == NOT_FOUND
        else:
            assert member.delete(
                f"{_issue(ws, pid_a, xb1)}/reactions/🍊/").status_code == 204

        # Comment reaction on C5 via A → 201 django / 404 rust.
        resp = member.post(
            f"{_base(ws, pid_a)}/comments/{c5}/reactions/",
            json={"reaction": "+1"},
        )
        assert resp.status_code == expect("DIFF-13", django=201, rust=404)
        if is_rust():
            assert resp.json() == NOT_FOUND
        else:
            assert member.delete(
                f"{_base(ws, pid_a)}/comments/{c5}/reactions/+1/"
            ).status_code == 204

        # Comment via A → 201 django / 404 rust.
        resp = member.post(
            f"{_issue(ws, pid_a, xb1)}/comments/",
            json={"comment_html": "<p>intruder</p>",
                  "comment_stripped": "intruder"},
        )
        assert resp.status_code == expect("DIFF-13", django=201, rust=404)
        if is_rust():
            assert resp.json() == NOT_FOUND
        else:
            assert member.delete(
                f"{_issue(ws, pid_a, xb1)}/comments/{resp.json()['id']}/"
            ).status_code == 204
        if is_rust():
            # Done when: no subscription row for the caller on Rust.
            # Contract runs are workerless (no consumer for either
            # backend's `.delay()`/queue), so on Django the comment's
            # auto-subscription never materialises here either; the
            # leak itself is pinned by the debugger probe on the
            # parity stack. On Rust the 404 fires before anything is
            # enqueued, so `0` guards the no-partial-write class.
            assert _subscriber_count(db_conn, xb1, seed["member"]) == 0
    finally:
        _teardown_world(db_conn, world)
