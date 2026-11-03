"""NULL-dimension exclusion for the estimate plot branch (PIDASHCONV-506).

``build_graph_plot`` (``apps/api/pi_dash/utils/analytics_plot.py:84-86``)
runs ``exclude(dimension__isnull=True)`` right after ``extract_axis``,
*before* the ``y_axis`` branch — so it covers the estimate path
(``:110-115``) exactly like the issue_count path pinned by PIDASHCONV-496.
Unlinked issues (no label, assignee, estimate, cycle, module) are therefore
dropped from the estimate ``distribution`` while still counting toward
``total``.

The seeded world has no linked rows at all, so the shape suite never
exercises this: it only queries ``priority``/``state_id``. This module adds
one more unlinked issue and pins the nullable axes (``labels__id``,
``assignees__id``) to the Django oracle: no ``"None"``/``"null"`` bucket —
an empty distribution — with ``total`` still counting the unlinked row.

Segment interplay (checked, not excluded): Python excludes only the
dimension (``:89-115`` has no segment exclude), so a segmented estimate
query keeps rows whose *segment* is NULL. The Rust builder mirrors this —
the ``IS NOT NULL`` guard applies to the dimension expression only.
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
            (NOW, NOW, issue_id, "I-NULL analytics estimate", "{}", "low", 9002,
             seed["admin"], seed["project"], seed["state_backlog"],
             seed["ws"], "", 0.0, 2, None, False, "", "", 0),
        )
    return issue_id


def _delete_issue(db_conn, issue_id):
    with db_conn.cursor() as cur:
        cur.execute("DELETE FROM issues WHERE id = %s", (issue_id,))


def test_estimate_nullable_axes_exclude_unlinked_issues(clients, seed, db_conn):
    issue_id = _insert_unlinked_issue(db_conn, seed)
    try:
        admin = clients["admin"]
        ws = seed["ws_slug"]
        for axis in ("labels__id", "assignees__id"):
            resp = admin.get(
                f"/api/workspaces/{ws}/analytics/?x_axis={axis}&y_axis=estimate"
            )
            assert resp.status_code == 200
            body = resp.json()
            # Every row is unlinked on these axes, so the oracle excludes
            # them all: empty distribution, but total still counts them.
            assert body["total"] == 4
            assert body["distribution"] == {}
            assert "None" not in body["distribution"]
            assert "null" not in body["distribution"]
    finally:
        _delete_issue(db_conn, issue_id)
