"""Cumulative token total on ``agent_status`` (PDASHOSS01-189).

``IssueDetailSerializer.get_agent_status`` renders ``total_tokens`` next
to ``run_count``: ``Sum("total_tokens")`` over every run on the issue,
``or 0``. ``total_tokens`` is the generated column over ``usage["total"]``,
so a run that never reported usage is NULL and the sum skips it.

Seeds its own pod, two issues and their runs and cleans them up, so the
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
    " usage, created_at, ended_at"
)

# (issue sequence, [usage per run]) — both issues live on project IS.
WORLD = {
    "tokens": (41, ['{"input": 11000, "output": 1000, "total": 12000}',
                    '{"input": 700, "output": 100, "total": 800}',
                    "{}"]),
    "silent": (42, ["{}"]),
}


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def _seed(cur, seed, world):
    ws, admin = seed["ws"], seed["admin"]
    project, state = seed["project"], seed["state_backlog"]
    # Not the project default, so no default-pod pick elsewhere sees it.
    cur.execute(
        "INSERT INTO pod (id, name, description, is_default,"
        " workspace_id, project_id, created_at, updated_at)"
        " VALUES (%s,%s,'',%s,%s,%s,%s,%s)",
        (world["pod"], "is-tokens", False, ws, project, NOW, NOW),
    )
    for key, (seq, usages) in WORLD.items():
        issue = world["issues"][key]
        cur.execute(
            "INSERT INTO issues (%s) VALUES (%s)" % (
                ISSUE_COLUMNS, ",".join(["%s"] * 19)),
            (NOW, NOW, issue, f"Tokens {key}", "{}", "medium", seq, admin,
             project, state, ws, "", 0.0, None, None, False, "", "", 0),
        )
        for usage in usages:
            cur.execute(
                "INSERT INTO agent_run (%s) VALUES (%s)" % (
                    RUN_COLUMNS, ",".join(["%s"] * 24)),
                (str(uuid.uuid4()), "completed", "contract prompt", "{}",
                 "{}", "contract-thread", "", ws, issue, world["pod"], admin,
                 "contract-model", "", "direct", "local_runner", 0, "", "",
                 "{}", "work", "{}", usage, NOW, NOW),
            )


def _cleanup(cur, world):
    for issue in world["issues"].values():
        cur.execute("DELETE FROM agent_run WHERE work_item_id = %s", (issue,))
        cur.execute(
            "DELETE FROM issue_sequences WHERE issue_id = %s", (issue,))
        cur.execute("DELETE FROM issues WHERE id = %s", (issue,))
    cur.execute("DELETE FROM pod WHERE id = %s", (world["pod"],))


def test_agent_status_total_tokens(clients, seed, db_conn):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    world = {
        "pod": str(uuid.uuid4()),
        "issues": {key: str(uuid.uuid4()) for key in WORLD},
    }
    issues = world["issues"]
    try:
        with db_conn.cursor() as cur:
            _seed(cur, seed, world)
        for key, expected_runs, expected_total in (
            ("tokens", 3, 12800),
            ("silent", 1, 0),
        ):
            seq = WORLD[key][0]
            for url in (
                f"{_base(ws, pid)}/issues/{issues[key]}/",
                f"/api/workspaces/{ws}/work-items/IS-{seq}/",
            ):
                resp = admin.get(url)
                assert resp.status_code == 200, url
                status = resp.json()["agent_status"]
                assert list(status) == [
                    "ticker", "active_run", "latest_run", "run_count",
                    "total_tokens",
                ]
                assert status["run_count"] == expected_runs
                # An int, never None: the NULL row is skipped, not summed.
                assert status["total_tokens"] == expected_total
                assert type(status["total_tokens"]) is int
    finally:
        with db_conn.cursor() as cur:
            _cleanup(cur, world)
