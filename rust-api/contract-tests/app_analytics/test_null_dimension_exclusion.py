"""NULL-dimension exclusion for non-date count axes (PIDASHCONV-496).

``build_graph_plot`` (``apps/api/pi_dash/utils/analytics_plot.py:84-86``)
runs ``exclude(dimension__isnull=True)`` for *every* axis — ``extract_axis``
always returns ``"dimension"``, so the ``if x_axis == "dimension"`` guard is
always true. Unlinked issues (no label, assignee, estimate, cycle, module)
are therefore dropped from the issue_count ``distribution`` while still
counting toward ``total``.

The seeded PIDASHCONV-93 world has no linked rows at all, so the shape suite
never exercises this: it only queries ``priority``/``state_id``. This module
adds one more unlinked issue and pins the nullable axes (``labels__id``,
``assignees__id``) to the Django oracle: no ``"None"``/``"null"`` bucket,
``total`` still counts the unlinked row.
"""

import uuid
from datetime import datetime, timezone

NOW = datetime.now(timezone.utc)


def _insert_unlinked_issue(db_conn, seed):
    issue_id = str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO issues (created_at, updated_at, id, name,"
            " description_json, priority, sequence_id, created_by_id, project_id,"
            " state_id, workspace_id, description_html, sort_order, point,"
            " completed_at, is_draft, git_work_branch, workpad, complexity_score)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, issue_id, "I-NULL analytics", "{}", "low", 9001,
             seed["admin"], seed["project"], seed["state_backlog"],
             seed["ws"], "", 0.0, 2, None, False, "", "", 0),
        )
    return issue_id


def _delete_issue(db_conn, issue_id):
    with db_conn.cursor() as cur:
        cur.execute("DELETE FROM issues WHERE id = %s", (issue_id,))


def _assert_no_none_bucket(body, total):
    assert body["total"] == total
    assert "None" not in body["distribution"]
    assert "null" not in body["distribution"]
    for buckets in body["distribution"].values():
        for bucket in buckets:
            assert set(bucket) == {"dimension", "count"}
            assert bucket["dimension"] not in ("None", "null", None)


def test_nullable_axes_exclude_unlinked_issues(clients, seed, db_conn):
    issue_id = _insert_unlinked_issue(db_conn, seed)
    try:
        admin = clients["admin"]
        ws = seed["ws_slug"]
        for axis in ("labels__id", "assignees__id"):
            resp = admin.get(
                f"/api/workspaces/{ws}/analytics/?x_axis={axis}&y_axis=issue_count"
            )
            assert resp.status_code == 200
            _assert_no_none_bucket(resp.json(), 4)
    finally:
        _delete_issue(db_conn, issue_id)
