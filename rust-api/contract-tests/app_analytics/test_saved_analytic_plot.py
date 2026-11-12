"""Saved-analytic-view plot parity (PIDASHCONV-1452).

``GET /api/workspaces/<slug>/saved-analytic-view/<id>/`` must build the
same distribution ``build_graph_plot``
(``apps/api/pi_dash/utils/analytics_plot.py:74-120``) gives on
``/analytics/`` for the same axes. The saved handler builds its plot in
its own ``saved_statements`` copy
(``rust-api/crates/api/src/app_analytics/render.rs``), which diverged
from the route-1 builders in three ways:

1. The issue_count outer SELECT never selects ``"segment"``, so every
   bucket renders ``"segment": null``.
2. NULL dimensions are guarded on the raw column for date axes only
   (dropping Django's ``"-"`` bucket) and not at all for relation axes
   or the estimate branch (keeping a ``"None"`` bucket Django excludes
   via ``exclude(dimension__isnull=True)`` at ``analytics_plot.py:84-86``).
3. Estimate sums render as JSON integers (``3``) instead of floats
   (``3.0``) because the rows go through ``row_to_json`` instead of
   ``render_estimate``.

Each test below fails on the current Rust, passes after the fix, and
passes on Django.
"""

import json
import uuid
from datetime import datetime, timezone

NOW = datetime.now(timezone.utc)


def _insert_view(db_conn, seed, x_axis, y_axis, name="AV-1452"):
    view_id = str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO analytic_views (created_at, updated_at, id, name,"
            " description, query, query_dict, created_by_id, workspace_id)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, view_id, name, "1452 probe",
             json.dumps({"workspace__slug": seed["ws_slug"]}),
             json.dumps({"x_axis": x_axis, "y_axis": y_axis}),
             seed["admin"], seed["ws"]),
        )
    return view_id


def _delete_view(db_conn, view_id):
    with db_conn.cursor() as cur:
        cur.execute("DELETE FROM analytic_views WHERE id = %s", (view_id,))


def _insert_unlinked_issue(db_conn, seed, name="I-1452 null"):
    issue_id = str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO issues (created_at, updated_at, id, name,"
            " description_json, priority, sequence_id, created_by_id, project_id,"
            " state_id, workspace_id, description_html, sort_order, point,"
            " completed_at, is_draft, git_work_branch, workpad, complexity_score)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, issue_id, name, "{}", "low", 9042,
             seed["admin"], seed["project"], seed["state_backlog"],
             seed["ws"], "", 0.0, 2, None, False, "", "", 0),
        )
    return issue_id


def _delete_issue(db_conn, issue_id):
    with db_conn.cursor() as cur:
        cur.execute("DELETE FROM issues WHERE id = %s", (issue_id,))


def test_saved_segmented_count_has_segment_values(clients, seed):
    """Segmented saved count plot fills segment with each issue's state group."""
    resp = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/saved-analytic-view/{seed['view']}/"
        "?segment=state__group"
    )
    assert resp.status_code == 200
    body = resp.json()
    assert body["total"] == 3
    assert body["distribution"] == {
        "high": [{"dimension": "high", "segment": "backlog", "count": 1}],
        "medium": [{"dimension": "medium", "segment": "completed", "count": 1}],
        "urgent": [{"dimension": "urgent", "segment": "backlog", "count": 1}],
    }
    for buckets in body["distribution"].values():
        for bucket in buckets:
            assert bucket["segment"] is not None


def test_saved_relation_axis_excludes_null_dimensions(clients, seed, db_conn):
    """Saved plots on assignees__id drop unassigned issues (no "None" bucket)."""
    issue_id = _insert_unlinked_issue(db_conn, seed)
    view_count = _insert_view(db_conn, seed, "assignees__id", "issue_count")
    view_est = _insert_view(db_conn, seed, "assignees__id", "estimate")
    try:
        admin = clients["admin"]
        ws = seed["ws_slug"]
        for view_id, value_key in ((view_count, "count"), (view_est, "estimate")):
            resp = admin.get(f"/api/workspaces/{ws}/saved-analytic-view/{view_id}/")
            assert resp.status_code == 200
            body = resp.json()
            assert body["total"] == 4
            assert body["distribution"] == {}
            assert "None" not in body["distribution"]
            assert "null" not in body["distribution"]
    finally:
        _delete_view(db_conn, view_est)
        _delete_view(db_conn, view_count)
        _delete_issue(db_conn, issue_id)


def test_saved_date_axis_keeps_missing_dates_as_dash(clients, seed, db_conn):
    """Saved plots on start_date keep NULL dates as the "-" bucket."""
    issue_id = _insert_unlinked_issue(db_conn, seed)
    view_id = _insert_view(db_conn, seed, "start_date", "issue_count")
    try:
        resp = clients["admin"].get(
            f"/api/workspaces/{seed['ws_slug']}/saved-analytic-view/{view_id}/"
        )
        assert resp.status_code == 200
        body = resp.json()
        assert body["total"] == 4
        assert body["distribution"] == {
            "-": [{"dimension": "-", "count": 4}],
        }
    finally:
        _delete_view(db_conn, view_id)
        _delete_issue(db_conn, issue_id)


def _seed_estimates(db_conn, seed):
    """Link I1/I2/I3 to whole-number point estimates; return cleanup data."""
    estimate_id = str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO estimates (id, name, description, project_id, workspace_id,"
            " type, last_used, created_at, updated_at)"
            " VALUES (%s, 'Est1452', '', %s, %s, 'points', false, now(), now())",
            (estimate_id, seed["project"], seed["ws"]),
        )
        point_ids = {}
        for value in ("5", "3", "8"):
            point_id = str(uuid.uuid4())
            point_ids[value] = point_id
            cur.execute(
                "INSERT INTO estimate_points (id, estimate_id, project_id, workspace_id,"
                " key, value, description, created_at, updated_at)"
                " VALUES (%s, %s, %s, %s, %s, %s, '', now(), now())",
                (point_id, estimate_id, seed["project"], seed["ws"], value, value),
            )
        for issue_key, value in (
            ("issue1", "5"), ("issue2", "3"), ("issue3", "8"),
        ):
            cur.execute(
                "UPDATE issues SET estimate_point_id = %s WHERE id = %s",
                (point_ids[value], seed[issue_key]),
            )
    return estimate_id, list(point_ids.values())


def _unseed_estimates(db_conn, seed, estimate_id, point_ids):
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET estimate_point_id = NULL WHERE id IN (%s,%s,%s)",
            (seed["issue1"], seed["issue2"], seed["issue3"]),
        )
        cur.execute(
            "DELETE FROM estimate_points WHERE id IN (%s,%s,%s)", tuple(point_ids)
        )
        cur.execute("DELETE FROM estimates WHERE id = %s", (estimate_id,))


def test_saved_estimate_renders_float(clients, seed, db_conn):
    """Saved estimate plots render whole-number sums as JSON floats (3.0)."""
    estimate_id, point_ids = _seed_estimates(db_conn, seed)
    view_id = _insert_view(db_conn, seed, "priority", "estimate")
    try:
        resp = clients["admin"].get(
            f"/api/workspaces/{seed['ws_slug']}/saved-analytic-view/{view_id}/"
        )
        assert resp.status_code == 200
        body = resp.json()
        assert body["total"] == 3
        assert body["distribution"] == {
            "high": [{"dimension": "high", "estimate": 5.0}],
            "medium": [{"dimension": "medium", "estimate": 8.0}],
            "urgent": [{"dimension": "urgent", "estimate": 3.0}],
        }
        # resp.json() cannot tell 3 from 3.0 (5 == 5.0); the raw body must
        # carry the float spelling Django emits for Sum(Cast(FloatField)).
        assert '"estimate":3.0' in resp.text
        assert '"estimate":5.0' in resp.text
        assert '"estimate":8.0' in resp.text
    finally:
        _delete_view(db_conn, view_id)
        _unseed_estimates(db_conn, seed, estimate_id, point_ids)
