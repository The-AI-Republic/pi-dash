"""project-stats row order is newest-first (PIDASHCONV-1454).

``GET /api/workspaces/<slug>/project-stats/`` runs
``Project.objects.filter(workspace__slug=slug)[.filter(id__in=…)]``
(``app/views/analytic/base.py:409-455``) with nothing clearing the model
ordering, so ``Project.Meta.ordering = ("-created_at",)``
(``db/models/project.py:253``) applies and the SQL ends in
``ORDER BY "projects"."created_at" DESC``.

The fixture adds a second project P2 to ``an-ws`` with ``created_at`` one
hour after the seed project's, then both tests assert the response id
order is ``[P2, seed]`` — plain and with ``?project_ids=`` given in the
opposite (oldest-first) order, proving the server sorts rather than
echoing the query.

NOTE (adjacent, out of scope): the per-row *key* order diverges —
Django renders ``total_cycles, total_modules`` before ``total_members``,
Rust renders ``total_members`` first. Assertions below compare id lists
(parsed JSON), so they pass on both backends despite that.
"""

import uuid
from datetime import timedelta

import pytest


@pytest.fixture()
def newer_project(db_conn, seed):
    with db_conn.cursor() as cur:
        cur.execute(
            "SELECT created_at FROM projects WHERE id = %s",
            (seed["project"],),
        )
        base = cur.fetchone()[0]
    pid = str(uuid.uuid4())
    created = base + timedelta(hours=1)
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO projects (created_at, updated_at, id, name, description,"
            " network, identifier, workspace_id, cycle_view, module_view,"
            " issue_views_view, page_view, intake_view, archive_in, close_in,"
            " logo_props, is_time_tracking_enabled, is_issue_type_enabled,"
            " guest_view_all_features, timezone, members_can_edit_states,"
            " repo_url, base_branch, agent_default_interval_seconds,"
            " agent_default_max_ticks, agent_ticking_enabled, is_default,"
            " agent_review_default_interval_seconds, default_agent_executor,"
            " agent_test_default_interval_seconds)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (created, created, pid, "Analytics Project Newer", "", 0, "AN-NEW",
             seed["ws"], True, True, True, True, True, 0, 0, "{}", True, True,
             True, "UTC", True, "", "", 3600, 100, True, False, 10800,
             "muse", 10800),
        )
    yield pid
    with db_conn.cursor() as cur:
        cur.execute("DELETE FROM projects WHERE id = %s", (pid,))


def test_project_stats_newest_first(clients, seed, newer_project):
    resp = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/project-stats/"
    )
    assert resp.status_code == 200
    assert [row["id"] for row in resp.json()] == [
        newer_project,
        seed["project"],
    ]


def test_project_stats_newest_first_with_project_ids(clients, seed, newer_project):
    # Query ids oldest-first; the server must still answer newest-first.
    resp = clients["admin"].get(
        f"/api/workspaces/{seed['ws_slug']}/project-stats/"
        f"?project_ids={seed['project']},{newer_project}"
    )
    assert resp.status_code == 200
    assert [row["id"] for row in resp.json()] == [
        newer_project,
        seed["project"],
    ]
