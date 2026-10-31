"""Merged issue-history read (PIDASHCONV-1680).

Ported from ``apps/api/pi_dash/tests/contract/api/test_issue_history.py``
(Django main @ b4defc50f): ``GET .../issues/<id>/history/`` without
``activity_type`` serializes the activities and the comments with their
own serializers, merges the plain dicts, and sorts ascending by the
rendered ``created_at`` (stable, so ties keep chain order: activities
first). Retires PB-0260 — the old read sorted raw model rows with
``instance["created_at"]`` and 500'd on any non-empty feed.

World: a private HIST project under the ``is-ws`` workspace (so no other
file's counts move) with one issue carrying an interleaved feed — a
creation entry, a comment, a state update, a later comment — plus an
empty issue.
"""

from datetime import datetime, timedelta, timezone
from urllib.parse import quote
import uuid

import pytest

PROJECT_COLUMNS = (
    "created_at, updated_at, id, name, description,"
    " network, identifier, workspace_id, cycle_view, module_view,"
    " issue_views_view, page_view, intake_view, archive_in, close_in,"
    " logo_props, is_time_tracking_enabled, is_issue_type_enabled,"
    " guest_view_all_features, timezone, members_can_edit_states,"
    " repo_url, base_branch, agent_default_interval_seconds,"
    " agent_default_max_ticks, agent_ticking_enabled, is_default,"
    " agent_review_default_interval_seconds, default_agent_executor,"
    " agent_test_default_interval_seconds"
)


def _history_url(ws_slug, project_id, issue_id, query=""):
    return (
        f"/api/workspaces/{ws_slug}/projects/{project_id}"
        f"/issues/{issue_id}/history/{query}"
    )


@pytest.fixture(scope="module")
def history_world(db_conn, seed):
    ws_id = seed["ws"]
    admin = seed["admin"]
    now = datetime.now(timezone.utc).replace(microsecond=0)
    base = now - timedelta(minutes=10)
    ids = {
        "project": str(uuid.uuid4()),
        "state": str(uuid.uuid4()),
        "issue": str(uuid.uuid4()),
        "empty_issue": str(uuid.uuid4()),
        "creation": str(uuid.uuid4()),
        "state_update": str(uuid.uuid4()),
        "first_comment": str(uuid.uuid4()),
        "second_comment": str(uuid.uuid4()),
        "base": base,
    }
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO projects (%s) VALUES (%s)" % (
                PROJECT_COLUMNS, ",".join(["%s"] * 30)),
            (now, now, ids["project"], "History Test Project", "", 0,
             "HIST", ws_id, True, True, True, True, True, 0, 0, "{}", True,
             True, True, "UTC", True, "", "", 3600, 100, True, False,
             10800, "muse", 10800),
        )
        cur.execute(
            "INSERT INTO project_members (created_at, updated_at, id, role,"
            " member_id, project_id, workspace_id, view_props, default_props,"
            " sort_order, preferences, is_active)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (now, now, str(uuid.uuid4()), 20, admin, ids["project"],
             ws_id, "{}", "{}", 0.0, "{}", True),
        )
        cur.execute(
            "INSERT INTO states (created_at, updated_at, id, name, description,"
            " color, slug, project_id, workspace_id, sequence, \"group\","
            " \"default\", is_triage)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (now, now, ids["state"], "HIST Backlog", "", "#000000",
             "hist-backlog", ids["project"], ws_id, 0.0, "backlog",
             True, False),
        )
        for key, name, seq in (("issue", "history target", 1),
                              ("empty_issue", "no activity", 2)):
            cur.execute(
                "INSERT INTO issues (created_at, updated_at, id, name,"
                " description_json, priority, sequence_id, created_by_id,"
                " project_id, state_id, workspace_id, description_html,"
                " sort_order, point, completed_at, is_draft, git_work_branch,"
                " workpad, complexity_score)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,"
                "%s,%s)",
                (now, now, ids[key], name, "{}", "high", seq, admin,
                 ids["project"], ids["state"], ws_id, "", 0.0, None,
                 None, False, "", "", 0),
            )
        # Interleaved feed: creation, comment, state update, comment.
        rows = [
            ("creation", None, None, None, base),
            ("state_update", "state", "Todo", "In Progress",
             base + timedelta(minutes=2)),
        ]
        for key, field, old, new, created in rows:
            cur.execute(
                "INSERT INTO issue_activities (created_at, updated_at, id,"
                " verb, field, old_value, new_value, created_by_id, issue_id,"
                " project_id, updated_by_id, workspace_id, actor_id, epoch,"
                " comment, attachments)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (created, created, ids[key], "updated", field, old, new,
                 admin, ids["issue"], ids["project"], admin, ws_id,
                 admin, int(created.timestamp()), "", "{}"),
            )
        comments = [
            ("first_comment", "first comment", base + timedelta(minutes=1)),
            ("second_comment", "second comment",
             base + timedelta(minutes=3)),
        ]
        for key, body, created in comments:
            cur.execute(
                "INSERT INTO issue_comments (created_at, updated_at, id,"
                " comment_html, comment_stripped, comment_json, created_by_id,"
                " issue_id, project_id, updated_by_id, workspace_id, actor_id,"
                " access, attachments, speaker_type, speaker_label, labels)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (created, created, ids[key], f"<p>{body}</p>", body, "{}",
                 admin, ids["issue"], ids["project"], admin, ws_id,
                 admin, "INTERNAL", "{}", "human", "", "{}"),
            )
    yield ids
    with db_conn.cursor() as cur:
        cur.execute(
            "DELETE FROM issue_activities WHERE id IN (%s,%s)",
            (ids["creation"], ids["state_update"]),
        )
        cur.execute(
            "DELETE FROM issue_comments WHERE id IN (%s,%s)",
            (ids["first_comment"], ids["second_comment"]),
        )
        cur.execute(
            "DELETE FROM issues WHERE id IN (%s,%s)",
            (ids["issue"], ids["empty_issue"]),
        )
        cur.execute("DELETE FROM states WHERE id = %s", (ids["state"],))
        cur.execute(
            "DELETE FROM project_members WHERE project_id = %s",
            (ids["project"],),
        )
        cur.execute("DELETE FROM projects WHERE id = %s", (ids["project"],))


def test_merged_read_returns_both_kinds_in_chronological_order(
    clients, seed, history_world
):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], history_world["project"]
    resp = admin.get(_history_url(ws, pid, history_world["issue"]))

    assert resp.status_code == 200
    payload = resp.json()
    assert len(payload) == 4

    created_at_values = [row["created_at"] for row in payload]
    assert created_at_values == sorted(created_at_values)

    activity_ids = {history_world["creation"], history_world["state_update"]}
    comment_ids = {
        history_world["first_comment"], history_world["second_comment"]}
    returned_ids = [row["id"] for row in payload]
    assert set(returned_ids) == activity_ids | comment_ids

    # The property rows and the comment rows are genuinely interleaved.
    kinds = [
        "activity" if row["id"] in activity_ids else "comment"
        for row in payload
    ]
    assert kinds == ["activity", "comment", "activity", "comment"]

    for row in payload:
        assert isinstance(row["id"], str)
        assert isinstance(row["created_at"], str)
        assert row["actor_detail"]["id"] == seed["admin"]


def test_merged_rows_match_the_split_read_shapes(clients, seed, history_world):
    admin = clients["admin"]
    ws, pid, iid = (
        seed["ws_slug"], history_world["project"], history_world["issue"])
    props = admin.get(
        _history_url(ws, pid, iid, "?activity_type=issue-property"))
    comments = admin.get(
        _history_url(ws, pid, iid, "?activity_type=issue-comment"))
    merged = admin.get(_history_url(ws, pid, iid))

    assert props.status_code == 200
    assert comments.status_code == 200
    assert merged.status_code == 200

    merged_by_id = {row["id"]: row for row in merged.json()}
    for row in props.json():
        assert merged_by_id[row["id"]] == row
    for row in comments.json():
        assert merged_by_id[row["id"]] == row


def test_merged_read_honours_created_at_gt_on_both_halves(
    clients, seed, history_world
):
    admin = clients["admin"]
    ws, pid, iid = (
        seed["ws_slug"], history_world["project"], history_world["issue"])
    cutoff = history_world["base"] + timedelta(minutes=1, seconds=30)
    resp = admin.get(
        _history_url(
            ws, pid, iid, f"?created_at__gt={quote(cutoff.isoformat())}"))

    assert resp.status_code == 200
    returned_ids = {row["id"] for row in resp.json()}
    assert returned_ids == {
        history_world["state_update"], history_world["second_comment"]}


def test_merged_read_with_no_entries_returns_an_empty_list(
    clients, seed, history_world
):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], history_world["project"]
    resp = admin.get(_history_url(ws, pid, history_world["empty_issue"]))

    assert resp.status_code == 200
    assert resp.json() == []


def test_merged_read_with_unknown_activity_type_returns_merged_feed(
    clients, seed, history_world
):
    # PB-0260's bogus-value clause: an unrecognized activity_type falls
    # through to the merged branch (200), exactly like the default read.
    admin = clients["admin"]
    ws, pid, iid = (
        seed["ws_slug"], history_world["project"], history_world["issue"])
    resp = admin.get(_history_url(ws, pid, iid, "?activity_type=bogus"))

    assert resp.status_code == 200
    assert {row["id"] for row in resp.json()} == {
        history_world["creation"], history_world["state_update"],
        history_world["first_comment"], history_world["second_comment"],
    }
