"""Contract tests: NEWPIDASH-239 archive disables scheduler bindings.

Archiving a project (v1 route) sets enabled=false on every live
scheduler binding of that project in the same transaction (bindings
stay installed, workspace definitions are untouched). Unarchiving does
not re-enable them. Run-now on an archived project answers the
NEWPIDASH-236 project_archived 409.

The ticker guards (scan + fire) are pinned by the Rust unit tests in
jobs/src/tasks_ticker; this suite pins the read/write behaviour
through the API.

All of this is new-system behaviour (DIFF-47): Django's archive stamps
``archived_at`` only, leaves ``scheduler_bindings.enabled`` alone and has
no archived check on run-now, so each case pins both arms with
``_harness.diffs.expect``.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import db, http  # noqa: E402
from _harness.diffs import expect  # noqa: E402

KEY = "projects"

DTSTART = "2026-10-01T00:00:00+00:00"
NEXT_RUN_AT = "2026-10-20T00:00:00+00:00"
FAR_NEXT_RUN_AT = "2099-01-01T00:00:00+00:00"
PROMPT = "Scan the project for stalled issues."


def _scheduler(conn, *, workspace_id, tag, created_by_id, slug=None):
    return db.insert_row(
        conn,
        "schedulers",
        {
            "workspace_id": workspace_id,
            "slug": slug or f"sched-{tag}",
            "name": f"CT Scheduler {tag}",
            "description": "Reads the project and files issues.",
            "prompt": PROMPT,
            "source": "builtin",
            "is_enabled": True,
            "color": "#3b82f6",
            "created_by_id": created_by_id,
        },
    )


def _binding(conn, *, workspace_id, project_id, scheduler_id, actor_id, **overrides):
    values = {
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
    }
    values.update(overrides)
    return db.insert_row(conn, "scheduler_bindings", values)


def _project(conn, ws_id, tag, owner_id):
    project = db.create_project(conn, ws_id, tag + "p", created_by_id=owner_id)
    db.add_project_member(conn, ws_id, project["id"], owner_id, db.ADMIN)
    return project


def _base(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects"


def _run_now_url(seed, project, scheduler):
    return (
        f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects/{project['id']}"
        f"/schedulers/{scheduler['id']}/run-now/"
    )


def _enabled(conn, binding_id):
    row = db.fetch_one(
        conn,
        "SELECT enabled, deleted_at FROM scheduler_bindings WHERE id = %s",
        (binding_id,),
    )
    assert row is not None, f"binding {binding_id} missing"
    return row["enabled"], row["deleted_at"]


def test_archive_disables_two_bindings_and_leaves_other_project(seed, conn):
    tag = db.new_tag()
    ws_id = seed["ws_a"]["id"]
    owner_id = seed["owner"]["id"]
    project = _project(conn, ws_id, tag, owner_id)
    other = _project(conn, ws_id, tag + "o", owner_id)
    sched_a = _scheduler(conn, workspace_id=ws_id, tag=tag + "a", created_by_id=owner_id)
    sched_b = _scheduler(conn, workspace_id=ws_id, tag=tag + "b", created_by_id=owner_id)
    sched_other = _scheduler(conn, workspace_id=ws_id, tag=tag + "c", created_by_id=owner_id)
    binding_a = _binding(conn, workspace_id=ws_id, project_id=project["id"], scheduler_id=sched_a["id"], actor_id=owner_id)
    binding_b = _binding(conn, workspace_id=ws_id, project_id=project["id"], scheduler_id=sched_b["id"], actor_id=owner_id)
    # Already-disabled binding on the same project stays disabled.
    disabled = _binding(conn, workspace_id=ws_id, project_id=project["id"], scheduler_id=sched_other["id"], actor_id=owner_id, enabled=False)
    other_binding = _binding(conn, workspace_id=ws_id, project_id=other["id"], scheduler_id=sched_other["id"], actor_id=owner_id)

    # `scheduler_bindings.enabled` of the archived project's live bindings
    # after archive (and after unarchive, which never re-enables): Django
    # leaves them on, Rust turns them off (DIFF-47).
    enabled_after_archive = expect("DIFF-47", django=True, rust=False)
    key = seed["keys"][KEY]
    http.post(key, f"{_base(seed)}/{project['id']}/archive/", expect=204)

    row = db.fetch_one(conn, "SELECT archived_at FROM projects WHERE id = %s", (project["id"],))
    assert row["archived_at"] is not None
    for binding in (binding_a, binding_b):
        enabled, deleted_at = _enabled(conn, binding["id"])
        assert enabled is enabled_after_archive, f"binding {binding['id']} enabled={enabled}"
        assert deleted_at is None, "binding must stay installed"
    enabled, deleted_at = _enabled(conn, disabled["id"])
    assert enabled is False and deleted_at is None
    enabled, deleted_at = _enabled(conn, other_binding["id"])
    assert enabled is True, "other project's binding must be untouched"
    # Workspace definitions are shared and untouched.
    for sched in (sched_a, sched_b):
        srow = db.fetch_one(conn, "SELECT is_enabled FROM schedulers WHERE id = %s", (sched["id"],))
        assert srow["is_enabled"] is True

    # Unarchive does not re-enable (owner decision 2026-11-13).
    http.delete(key, f"{_base(seed)}/{project['id']}/archive/", expect=204)
    row = db.fetch_one(conn, "SELECT archived_at FROM projects WHERE id = %s", (project["id"],))
    assert row["archived_at"] is None
    for binding in (binding_a, binding_b):
        enabled, _ = _enabled(conn, binding["id"])
        assert enabled is enabled_after_archive, "unarchive must not re-enable"


def test_run_now_on_archived_project_is_project_archived(seed, conn):
    tag = db.new_tag()
    ws_id = seed["ws_a"]["id"]
    owner_id = seed["owner"]["id"]
    project = _project(conn, ws_id, tag + "r", owner_id)
    sched = _scheduler(conn, workspace_id=ws_id, tag=tag + "r", created_by_id=owner_id)
    _binding(conn, workspace_id=ws_id, project_id=project["id"], scheduler_id=sched["id"], actor_id=owner_id)

    key = seed["keys"][KEY]
    # Sanity: run-now works while the project is active.
    http.post(key, _run_now_url(seed, project, sched), expect=200)
    http.post(key, f"{_base(seed)}/{project['id']}/archive/", expect=204)
    # Rust: archive disables the binding, but run-now must answer
    # project_archived (NEWPIDASH-236 envelope), not binding_disabled.
    # Django has no archived check and the binding is still enabled, so
    # run-now marks it due again (DIFF-47).
    archived_status = expect("DIFF-47", django=200, rust=409)
    # Push the binding's next run far out, so a 200 has to prove it
    # marked the archived binding due again (the sanity call above
    # already made it due once).
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE scheduler_bindings SET next_run_at = %s WHERE project_id = %s",
            (FAR_NEXT_RUN_AT, project["id"]),
        )
    conn.commit()
    with http.client(key) as c:
        r = c.post(_run_now_url(seed, project, sched))
    assert r.status_code == archived_status, r.text[:400]
    body = r.json()
    if archived_status == 200:
        assert body["enabled"] is True
        row = db.fetch_one(
            conn,
            "SELECT next_run_at <= now() AS due FROM scheduler_bindings WHERE project_id = %s",
            (project["id"],),
        )
        assert row["due"] is True, "Django run-now must mark the archived binding due"
    else:
        assert body["code"] == "project_archived"
        assert body["project"] == project["identifier"]
        assert "archived_at" in body
    # Re-enabling by hand does not rescue it either.
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE scheduler_bindings SET enabled = true WHERE project_id = %s",
            (project["id"],),
        )
    conn.commit()
    with http.client(key) as c:
        r = c.post(_run_now_url(seed, project, sched))
    assert r.status_code == archived_status, r.text[:400]
    if archived_status == 409:
        assert r.json()["code"] == "project_archived"

