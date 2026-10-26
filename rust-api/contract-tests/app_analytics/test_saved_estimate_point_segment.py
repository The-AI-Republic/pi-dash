"""Saved estimate plot segmented by estimate_point__value (PIDASHCONV-1501).

``GET /api/workspaces/<slug>/saved-analytic-view/<id>/?segment=estimate_point__value``
with ``y_axis=estimate`` must return the same distribution
``build_graph_plot`` (``apps/api/pi_dash/utils/analytics_plot.py:110-115``)
gives on ``/analytics/`` for the same axes: per-dimension buckets carrying
the segment value and a float estimate sum.

The saved handler builds its plot in its own ``saved_statements`` copy
(``rust-api/crates/api/src/app_analytics/render.rs``): ``already`` is true
when EITHER the dimension or the segment join mentions
``estimate_points``, so the base estimate join is suppressed when the
segment brings it — but the segment join is then zeroed exactly for
``segment == "estimate_point__value"``. Net: with
``x_axis != estimate_point__value`` the estimate table is joined zero
times while the SELECT list and the SUM reference
``"estimate_points"."value"`` → Postgres "missing FROM-clause entry" →
500 where Django returns 200. The route-1 builder
(``base_plot_estimate_sql``) checks only ``dim_join`` before zeroing, so
it always joins exactly once.

The test below fails (500) on the current Rust, passes after the fix,
and passes on Django.
"""

import json
import uuid
from datetime import datetime, timezone

NOW = datetime.now(timezone.utc)


def _insert_view(db_conn, seed, x_axis, y_axis, name="AV-1501"):
    view_id = str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO analytic_views (created_at, updated_at, id, name,"
            " description, query, query_dict, created_by_id, workspace_id)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, view_id, name, "1501 probe",
             json.dumps({"workspace__slug": seed["ws_slug"]}),
             json.dumps({"x_axis": x_axis, "y_axis": y_axis}),
             seed["admin"], seed["ws"]),
        )
    return view_id


def _delete_view(db_conn, view_id):
    with db_conn.cursor() as cur:
        cur.execute("DELETE FROM analytic_views WHERE id = %s", (view_id,))


def _seed_estimates(db_conn, seed):
    """Link I1/I2/I3 to whole-number point estimates; return cleanup data."""
    estimate_id = str(uuid.uuid4())
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO estimates (id, name, description, project_id, workspace_id,"
            " type, last_used, created_at, updated_at)"
            " VALUES (%s, 'Est1501', '', %s, %s, 'points', false, now(), now())",
            (estimate_id, seed["project"], seed["ws"]),
        )
        point_ids = {}
        for key, value in ((5, "5"), (3, "3"), (8, "8")):
            point_id = str(uuid.uuid4())
            point_ids[value] = point_id
            cur.execute(
                "INSERT INTO estimate_points (id, estimate_id, project_id, workspace_id,"
                " key, value, description, created_at, updated_at)"
                " VALUES (%s, %s, %s, %s, %s, %s, '', now(), now())",
                (point_id, estimate_id, seed["project"], seed["ws"], key, value),
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


def test_saved_estimate_segmented_by_estimate_point(clients, seed, db_conn):
    """Saved estimate plots segmented by estimate_point__value keep the join."""
    estimate_id, point_ids = _seed_estimates(db_conn, seed)
    view_id = _insert_view(db_conn, seed, "priority", "estimate")
    try:
        resp = clients["admin"].get(
            f"/api/workspaces/{seed['ws_slug']}/saved-analytic-view/{view_id}/"
            "?segment=estimate_point__value"
        )
        assert resp.status_code == 200
        body = resp.json()
        assert body["total"] == 3
        assert body["distribution"] == {
            "high": [{"dimension": "high", "segment": "5", "estimate": 5.0}],
            "medium": [{"dimension": "medium", "segment": "8", "estimate": 8.0}],
            "urgent": [{"dimension": "urgent", "segment": "3", "estimate": 3.0}],
        }
        # resp.json() cannot tell 3 from 3.0 (3 == 3.0); the raw body must
        # carry the float spelling Django emits for Sum(Cast(FloatField)).
        assert '"estimate":3.0' in resp.text
        assert '"estimate":5.0' in resp.text
        assert '"estimate":8.0' in resp.text
    finally:
        _delete_view(db_conn, view_id)
        _unseed_estimates(db_conn, seed, estimate_id, point_ids)
