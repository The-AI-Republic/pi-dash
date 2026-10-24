"""Issue-detail agent-run live_state carries the watchdog keys (PIDASHCONV-1315).

Django main ``b790ee7fa`` added ``agent_health`` /
``agent_last_activity_at`` to the issue-detail ``live_state`` block
(``IssueDetailSerializer._serialize_agent_live_state``), after
``turn_count`` and before ``updated_at``.

Seeds its own pod + runner + RUNNING run + live-state row on the
shared seed issue IS-1 and cleans them up, so the session seed is
untouched.
"""

import time
import uuid
from datetime import datetime, timezone

NOW = datetime.now(timezone.utc)

EXPECTED_LIVE_STATE_KEYS = [
    "observed_run_id",
    "last_event_at",
    "last_event_kind",
    "last_event_summary",
    "agent_pid",
    "agent_subprocess_alive",
    "approvals_pending",
    "input_tokens",
    "output_tokens",
    "total_tokens",
    "llm_model",
    "turn_count",
    "agent_health",
    "agent_last_activity_at",
    "updated_at",
]


def _seed_world(cur, seed):
    """Pod + runner + RUNNING run + live-state row on IS-1."""
    ws, admin, project = seed["ws"], seed["admin"], seed["project"]
    issue = seed["issue1"]
    pod_id, runner_id, run_id = (str(uuid.uuid4()) for _ in range(3))
    cur.execute(
        "INSERT INTO pod (id, name, description, is_default,"
        " workspace_id, project_id, created_at, updated_at)"
        " VALUES (%s,%s,'',true,%s,%s,%s,%s)",
        (pod_id, "is-agent-health", ws, project, NOW, NOW),
    )
    cur.execute(
        "INSERT INTO runner (id, owner_id, workspace_id, pod_id, name,"
        " host_label, provisioning, visibility, status, capabilities,"
        " os, arch, runner_version, refresh_token_hash,"
        " refresh_token_fingerprint, refresh_token_generation,"
        " previous_refresh_token_hash, access_token_signing_key_version,"
        " enrollment_token_hash, enrollment_token_fingerprint,"
        " enrolled_at, revoked_at, revoked_reason, dev_machine_id,"
        " dev_metadata, protocol_version, created_at, updated_at)"
        " VALUES (%s,%s,%s,%s,'is-health-runner','is-health-host','manual',"
        "0,'busy','[]','','','','','',0,'',1,'','',NULL,NULL,'',NULL,'{}',"
        "1,%s,%s)",
        (runner_id, admin, ws, pod_id, NOW, NOW),
    )
    cur.execute(
        "INSERT INTO agent_run (id, workspace_id, owner_id, created_by_id,"
        " pod_id, runner_id, work_item_id, status, prompt, trigger,"
        " executor_kind, run_config, required_capabilities, thread_id,"
        " agent_metadata, tool_plan, usage, dispatch_attempts, error,"
        " refusal_category, failure_reason, llm_model, cancel_reason,"
        " error_code, phase_kind, created_at)"
        " VALUES (%s,%s,%s,%s,%s,%s,%s,'running','','direct',"
        "'local_runner','{}','[]','{}','{}','{}','{}',0,'','','','','','',"
        "'',%s)",
        (run_id, ws, admin, admin, pod_id, runner_id, issue, NOW),
    )
    cur.execute(
        "INSERT INTO runner_live_state (runner_id, observed_run_id,"
        " last_event_at, last_event_kind, approvals_pending, usage,"
        " llm_model, turn_count, agent_health, agent_last_activity_at,"
        " updated_at)"
        " VALUES (%s,%s,'2026-10-21 04:05:06+00:00'::timestamptz,'tool',"
        " 0,'{}','m',3,'healthy',"
        " '2026-10-20 01:02:03+00:00'::timestamptz,%s)",
        (runner_id, run_id, NOW),
    )
    return {"pod": pod_id, "runner": runner_id, "run": run_id}


def _cleanup_world(cur, world, issue_id):
    # The detail GET fires recent_visited_task (async on Django): wait
    # for the visit row to land, then remove it — _wipe_world does not
    # cover user_recent_visits and its FK would break the next session.
    deadline = time.monotonic() + 20.0
    while time.monotonic() < deadline:
        cur.execute(
            "SELECT id FROM user_recent_visits WHERE entity_name = 'issue'"
            " AND entity_identifier = %s",
            (issue_id,),
        )
        if cur.fetchone():
            break
        time.sleep(0.5)
    cur.execute(
        "DELETE FROM user_recent_visits WHERE entity_name = 'issue'"
        " AND entity_identifier = %s",
        (issue_id,),
    )
    cur.execute("DELETE FROM runner_live_state WHERE runner_id = %s", (world["runner"],))
    cur.execute("DELETE FROM agent_run WHERE id = %s", (world["run"],))
    cur.execute("DELETE FROM runner WHERE id = %s", (world["runner"],))
    cur.execute("DELETE FROM pod WHERE id = %s", (world["pod"],))


def test_issue_detail_live_state_watchdog_keys(clients, seed, db_conn):
    admin = clients["admin"]
    with db_conn.cursor() as cur:
        world = _seed_world(cur, seed)
    try:
        resp = admin.get(f"/api/workspaces/{seed['ws_slug']}/work-items/IS-1/")
        assert resp.status_code == 200, resp.text
        body = resp.json()
        active = body["agent_status"]["active_run"]
        assert active is not None
        assert active["id"] == world["run"]
        live = active["live_state"]
        assert list(live) == EXPECTED_LIVE_STATE_KEYS
        assert live["agent_health"] == "healthy"
        assert live["agent_last_activity_at"].startswith("2026-10-20T01:02:03")
        assert live["last_event_at"].startswith("2026-10-21T04:05:06")
    finally:
        with db_conn.cursor() as cur:
            _cleanup_world(cur, world, seed["issue1"])
