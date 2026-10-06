"""Immediate-handoff move (PIDASHCONV-743).

A move whose handoff runs are all QUEUED (none executing) retires the
parent in-span and creates a fresh target-project handoff run through
the D-12 creation driver: 200, fresh row (parent linkage, QUEUED,
target default pod, rendered prompt), parent CANCELLED.

Seeds its own issue + pods + parent run and cleans them up, so the
shared session seed is untouched.
"""

import uuid
from datetime import datetime, timezone

NOW = datetime.now(timezone.utc)

ISSUE_COLUMNS = (
    "created_at, updated_at, id, name,"
    " description_json, priority, sequence_id, created_by_id, project_id,"
    " state_id, workspace_id, description_html, sort_order, point,"
    " completed_at, is_draft, git_work_branch, workpad, complexity_score"
)

RUN_COLUMNS = (
    "id, status, prompt, run_config, required_capabilities, thread_id,"
    " error, workspace_id, work_item_id, pod_id, created_by_id, llm_model,"
    " refusal_category, trigger, executor_kind, dispatch_attempts,"
    " cancel_reason, error_code, tool_plan, phase_kind, agent_metadata,"
    " usage, created_at"
)


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def _seed_handoff_world(cur, seed):
    """Insert target/source default pods, issue HX on IS2, QUEUED parent run."""
    ws, admin = seed["ws"], seed["admin"]
    src_project, src_state = seed["project2"], seed["state2_backlog"]
    tgt_project = seed["project"]
    src_pod, tgt_pod = str(uuid.uuid4()), str(uuid.uuid4())
    hx = str(uuid.uuid4())
    parent = str(uuid.uuid4())
    # Idempotency: an interrupted run may have left is- pods behind, and
    # the default-pod pick is ORDER BY created_at — stale rows would win.
    cur.execute(
        "DELETE FROM pod WHERE project_id IN (%s, %s)",
        (src_project, tgt_project),
    )
    for pod_id, project_id, name in (
        (src_pod, src_project, "is-handoff-src"),
        (tgt_pod, tgt_project, "is-handoff-tgt"),
    ):
        cur.execute(
            "INSERT INTO pod (id, name, description, is_default,"
            " workspace_id, project_id, created_at, updated_at)"
            " VALUES (%s,%s,'',%s,%s,%s,%s,%s)",
            (pod_id, name, True, ws, project_id, NOW, NOW),
        )
    cur.execute(
        "INSERT INTO issues (%s) VALUES (%s)" % (
            ISSUE_COLUMNS, ",".join(["%s"] * 19)),
        (NOW, NOW, hx, "HX handoff", "{}", "medium", 21, admin,
         src_project, src_state, ws, "", 0.0, None, None, False,
         "", "", 0),
    )
    cur.execute(
        "INSERT INTO agent_run (%s) VALUES (%s)" % (
            RUN_COLUMNS, ",".join(["%s"] * 23)),
        (parent, "queued", "contract parent prompt", "{}", "{}",
         "contract-thread", "", ws, hx, src_pod, admin, "contract-model",
         "", "direct", "local_runner", 0, "", "", "{}", "work", "{}",
         '{"input": 0, "output": 0, "total": 0}', NOW),
    )
    return {"src_pod": src_pod, "tgt_pod": tgt_pod, "hx": hx,
            "parent": parent}


def _cleanup_handoff_world(cur, world):
    cur.execute(
        "DELETE FROM agent_run WHERE work_item_id = %s", (world["hx"],))
    cur.execute(
        "DELETE FROM issue_sequences WHERE issue_id = %s", (world["hx"],))
    cur.execute("DELETE FROM issues WHERE id = %s", (world["hx"],))
    cur.execute(
        "DELETE FROM pod WHERE id IN (%s, %s)",
        (world["src_pod"], world["tgt_pod"]),
    )


def test_move_creates_immediate_handoff_run(clients, seed, db_conn):
    admin = clients["admin"]
    ws, tgt = seed["ws_slug"], seed["project"]
    src = seed["project2"]
    with db_conn.cursor() as cur:
        world = _seed_handoff_world(cur, seed)
    try:
        resp = admin.post(
            f"{_base(ws, src)}/work-items/{world['hx']}/move/",
            json={"project": "IS"})
        assert resp.status_code == 200
        body = resp.json()
        assert body["id"] == world["hx"]
        assert body["project_id"] == tgt
        assert body["name"] == "HX handoff"
        with db_conn.cursor() as cur:
            cur.execute(
                "SELECT status FROM agent_run WHERE id = %s",
                (world["parent"],))
            assert cur.fetchone()[0] == "cancelled"
            cur.execute(
                "SELECT id, status, pod_id, work_item_id, parent_run_id,"
                " trigger, prompt, executor_kind"
                " FROM agent_run WHERE parent_run_id = %s",
                (world["parent"],))
            rows = cur.fetchall()
            assert len(rows) == 1
            fresh = rows[0]
            assert fresh[1] == "queued"
            assert str(fresh[2]) == world["tgt_pod"]
            assert str(fresh[3]) == world["hx"]
            assert fresh[5] == "direct"
            assert fresh[6] not in (None, "")
    finally:
        with db_conn.cursor() as cur:
            _cleanup_handoff_world(cur, world)
