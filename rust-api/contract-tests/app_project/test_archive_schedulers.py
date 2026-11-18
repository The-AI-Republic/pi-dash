"""Contract: NEWPIDASH-239 app archive disables scheduler bindings.

Companion to v1_projects/test_archive_schedulers.py: same-transaction
disable on the app route (POST /api/workspaces/<slug>/projects/<id>/
archive/), bindings stay installed, unarchive does not re-enable.
"""

from psycopg.rows import dict_row

DTSTART = "2026-10-01T00:00:00+00:00"
NEXT_RUN_AT = "2026-10-20T00:00:00+00:00"
PROMPT = "Scan the project for stalled issues."


def _scheduler(db, *, workspace_id, tag, user_id):
    from _harness import db as dbmod

    return dbmod.insert_row(
        db,
        "schedulers",
        {
            "workspace_id": workspace_id,
            "slug": f"sched-{tag}",
            "name": f"CT Scheduler {tag}",
            "description": "Reads the project and files issues.",
            "prompt": PROMPT,
            "source": "builtin",
            "is_enabled": True,
            "color": "#3b82f6",
            "created_by_id": user_id,
        },
    )


def _binding(db, *, workspace_id, project_id, scheduler_id, actor_id):
    from _harness import db as dbmod

    return dbmod.insert_row(
        db,
        "scheduler_bindings",
        {
            "workspace_id": workspace_id,
            "project_id": project_id,
            "scheduler_id": scheduler_id,
            "dtstart": DTSTART,
            "tzid": "UTC",
            "rrule": "FREQ=HOURLY",
            "rdates": [],
            "exdates": [],
            "extra_context": "",
            "enabled": True,
            "outcome_mode": "create_issue",
            "next_run_at": NEXT_RUN_AT,
            "last_error": "",
            "actor_id": actor_id,
        },
    )


def test_app_archive_disables_bindings(world, db):
    client, user, ws, project = world.full_stack()
    tag = __import__("uuid").uuid4().hex[:8]
    sched_a = _scheduler(db, workspace_id=str(ws["id"]), tag=tag + "a", user_id=str(user["id"]))
    sched_b = _scheduler(db, workspace_id=str(ws["id"]), tag=tag + "b", user_id=str(user["id"]))
    binding_a = _binding(db, workspace_id=str(ws["id"]), project_id=str(project["id"]), scheduler_id=sched_a["id"], actor_id=str(user["id"]))
    binding_b = _binding(db, workspace_id=str(ws["id"]), project_id=str(project["id"]), scheduler_id=sched_b["id"], actor_id=str(user["id"]))

    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/archive/"
    resp = client.post(url)
    assert resp.status_code == 200, resp.text[:500]
    assert list(resp.json().keys()) == ["archived_at"]
    with db.cursor(row_factory=dict_row) as cur:
        for binding in (binding_a, binding_b):
            cur.execute("SELECT enabled, deleted_at FROM scheduler_bindings WHERE id=%s", (binding["id"],))
            row = cur.fetchone()
            assert row["enabled"] is False
            assert row["deleted_at"] is None
        cur.execute("SELECT is_enabled FROM schedulers WHERE id=%s", (sched_a["id"],))
        assert cur.fetchone()["is_enabled"] is True
        cur.execute("SELECT archived_at FROM projects WHERE id=%s", (str(project["id"]),))
        assert cur.fetchone()["archived_at"] is not None

    resp = client.delete(url)
    assert resp.status_code == 204
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT archived_at FROM projects WHERE id=%s", (str(project["id"]),))
        assert cur.fetchone()["archived_at"] is None
        for binding in (binding_a, binding_b):
            cur.execute("SELECT enabled FROM scheduler_bindings WHERE id=%s", (binding["id"],))
            assert cur.fetchone()["enabled"] is False
