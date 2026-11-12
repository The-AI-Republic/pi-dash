"""PIDASHCONV-1184: cycle/archive burndown completion buckets follow the actor zone.

Django buckets completions with ``TruncDate("completed_at")`` and no
explicit ``tzinfo``: ``TruncBase.get_tzname()`` uses the active zone, which
``TimezoneMixin`` sets to ``request.user.user_timezone`` for authenticated
requests (``TIME_ZONE`` is only the fallback). The four cycle/archive
burndown queries must bind that zone, not hard-code UTC. The range
endpoints and "today" stay UTC, as Django does.

Each test seeds one tenant whose actor zone is ``America/Los_Angeles``
(UTC-7 in September) with two estimated issues completed on either side of
UTC midnight. In UTC the completions land on 2026-09-15/16; in the actor's
zone they land on 2026-09-14/15, so the completion chart shifts by a day.
"""

import uuid

import pytest

from .conftest import cycle_urls

pytestmark = pytest.mark.contract

ACTOR_TZ = "America/Los_Angeles"

# Fixed range in the past so both dates are charted (no future `null`s).
START = "2026-09-10T00:00:00Z"
END = "2026-09-16T00:00:00Z"

# (completed_at, estimate value). A is 2026-09-14 23:00 in Los Angeles,
# B is 2026-09-15 20:00 there; in UTC they are 09-15 06:00 and 09-16 03:00.
COMPLETIONS = [("2026-09-15T06:00:00Z", "1"), ("2026-09-16T03:00:00Z", "3")]

# total_issues = 2, total points = 4; actor-zone buckets shift the step down
# one day versus the UTC buckets (14 vs 15, and 15 vs 16).
EXPECTED_ISSUES = {
    "2026-09-10": 2,
    "2026-09-11": 2,
    "2026-09-12": 2,
    "2026-09-13": 2,
    "2026-09-14": 1,
    "2026-09-15": 0,
    "2026-09-16": 0,
}
EXPECTED_POINTS = {
    "2026-09-10": 4.0,
    "2026-09-11": 4.0,
    "2026-09-12": 4.0,
    "2026-09-13": 4.0,
    "2026-09-14": 3.0,
    "2026-09-15": 0.0,
    "2026-09-16": 0.0,
}


def _seed(tenant):
    """Seed the estimate, cycle, completed issues and bridge rows."""
    conn = tenant["seed"].conn
    ws = tenant["workspace"]["id"]
    project = tenant["project"]["id"]
    user = tenant["user"]["id"]
    state = tenant["state"]["id"]

    with conn.cursor() as cur:
        cur.execute(
            "UPDATE users SET user_timezone = %s WHERE id = %s", (ACTOR_TZ, user)
        )
        estimate_id = str(uuid.uuid4())
        cur.execute(
            "INSERT INTO estimates (id, name, description, project_id, workspace_id,"
            " type, last_used, created_at, updated_at)"
            " VALUES (%s, 'Burndown', '', %s, %s, 'points', false, now(), now())",
            (estimate_id, project, ws),
        )
        cur.execute(
            "UPDATE projects SET estimate_id = %s WHERE id = %s", (estimate_id, project)
        )
        point_ids = {}
        for key, value in ((1, "1"), (3, "3")):
            point_id = str(uuid.uuid4())
            point_ids[value] = point_id
            cur.execute(
                "INSERT INTO estimate_points (id, estimate_id, project_id, workspace_id,"
                " key, value, description, created_at, updated_at)"
                " VALUES (%s, %s, %s, %s, %s, %s, '', now(), now())",
                (point_id, estimate_id, project, ws, key, value),
            )
        cycle_id = str(uuid.uuid4())
        cur.execute(
            "INSERT INTO cycles (id, name, description, start_date, end_date, owned_by_id,"
            " project_id, workspace_id, view_props, sort_order, external_source, external_id,"
            " progress_snapshot, archived_at, logo_props, timezone, version, created_by_id,"
            " created_at, updated_at) VALUES"
            " (%s, 'Burndown', '', %s, %s, %s, %s, %s, '{}', 65535, null, null, '{}', null,"
            " '{}', 'UTC', 1, %s, now(), now())",
            (cycle_id, START, END, user, project, ws, user),
        )
        for index, (completed_at, value) in enumerate(COMPLETIONS):
            issue_id = str(uuid.uuid4())
            cur.execute(
                "INSERT INTO issues (id, name, description_json, priority, sequence_id,"
                " project_id, workspace_id, state_id, description_html, sort_order, is_draft,"
                " git_work_branch, workpad, complexity_score, created_at, updated_at,"
                " created_by_id, completed_at, estimate_point_id) VALUES"
                " (%s, %s, '{}', 'high', %s, %s, %s, %s, '<p></p>', 65535, false, '', '', 0,"
                " now(), now(), %s, %s, %s)",
                (
                    issue_id,
                    f"Burndown {index}",
                    9100 + index,
                    project,
                    ws,
                    state,
                    user,
                    completed_at,
                    point_ids[value],
                ),
            )
            cur.execute(
                "INSERT INTO cycle_issues (id, created_at, updated_at, cycle_id, issue_id,"
                " project_id, workspace_id, created_by_id)"
                " VALUES (%s, now(), now(), %s, %s, %s, %s, %s)",
                (str(uuid.uuid4()), cycle_id, issue_id, project, ws, user),
            )
    return cycle_id


def _cleanup(tenant, cycle_id):
    conn = tenant["seed"].conn
    project = tenant["project"]["id"]
    with conn.cursor() as cur:
        cur.execute("DELETE FROM cycle_issues WHERE cycle_id = %s", (cycle_id,))
        cur.execute(
            "DELETE FROM issues WHERE project_id = %s AND estimate_point_id IS NOT NULL",
            (project,),
        )
        cur.execute(
            "UPDATE projects SET estimate_id = NULL WHERE id = %s", (project,)
        )
        cur.execute("DELETE FROM cycles WHERE id = %s", (cycle_id,))
        cur.execute("DELETE FROM estimate_points WHERE project_id = %s", (project,))
        cur.execute("DELETE FROM estimates WHERE project_id = %s", (project,))


def test_analytics_completion_chart_actor_timezone(make_tenant):
    tenant = make_tenant()
    cycle_id = _seed(tenant)
    urls = cycle_urls(tenant)
    try:
        r = tenant["client"].get(urls["analytics"](cycle_id) + "?type=issues")
        assert r.status_code == 200, r.text[:500]
        assert r.json()["completion_chart"] == EXPECTED_ISSUES

        r = tenant["client"].get(urls["analytics"](cycle_id) + "?type=points")
        assert r.status_code == 200, r.text[:500]
        assert r.json()["completion_chart"] == EXPECTED_POINTS
    finally:
        _cleanup(tenant, cycle_id)


def test_archived_completion_chart_actor_timezone(make_tenant):
    tenant = make_tenant()
    cycle_id = _seed(tenant)
    urls = cycle_urls(tenant)
    try:
        r = tenant["client"].post(urls["archive"](cycle_id))
        assert r.status_code == 200, r.text[:500]

        r = tenant["client"].get(urls["archived_detail"](cycle_id))
        assert r.status_code == 200, r.text[:500]
        body = r.json()
        assert body["distribution"]["completion_chart"] == EXPECTED_ISSUES
        assert body["estimate_distribution"]["completion_chart"] == EXPECTED_POINTS
    finally:
        _cleanup(tenant, cycle_id)
