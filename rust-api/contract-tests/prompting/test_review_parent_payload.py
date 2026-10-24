"""Review preview escapes the parent done payload like ``ensure_ascii``.

Django's ``_parent_done_payload``
(``apps/api/pi_dash/prompting/context.py``) renders
``json.dumps(payload, indent=2, sort_keys=True)`` with the default
``ensure_ascii=True``: every non-ASCII char becomes a lowercase
``\\uXXXX`` escape (surrogate pairs above the BMP). The review-kind
preview of an issue whose parent run carries a non-ASCII done payload
must show that escaped form, never raw UTF-8 (PIDASHCONV-1234).
"""

import uuid
from datetime import datetime, timezone

from psycopg.types.json import Json


def _preview_url(tenant):
    return (f"/api/workspaces/{tenant['workspace']['slug']}"
            f"/prompts/review/preview")


def _seed_parent_run(seed, workspace_id, project_id, issue_id, user_id,
                     payload):
    """Completed parent run + ticker resume pointer (raw SQL).

    Returns ``(run_id, ticker_id, pod_id)``. The caller must delete all
    three rows before teardown: the suite sweep deletes workspace
    issues/projects first, which the run's ``work_item`` FK and the
    pod's ``project`` FK would block.
    """
    pod_id = seed.pod(workspace_id, project_id)
    run_id = str(uuid.uuid4())
    ticker_id = str(uuid.uuid4())
    now = datetime.now(timezone.utc)
    with seed.conn.cursor() as cur:
        cur.execute(
            """INSERT INTO agent_run
               (id, status, prompt, run_config, required_capabilities,
                thread_id, error, workspace_id, pod_id, created_by_id,
                work_item_id, llm_model, refusal_category, trigger,
                executor_kind, dispatch_attempts, cancel_reason,
                error_code, tool_plan, phase_kind, agent_metadata, usage,
                done_payload, created_at)
               VALUES (%s, 'completed', '', '{}', '{}', '', '', %s, %s,
                       %s, %s, '', '', 'tick', 'local_runner', 0, '', '',
                       '{}', '', '{}', '{}', %s, %s)""",
            (run_id, workspace_id, pod_id, user_id, issue_id,
             Json(payload), now),
        )
        cur.execute(
            """INSERT INTO issue_agent_ticker
               (id, issue_id, user_disabled, used, enabled, next_run_at,
                last_tick_at, disarm_reason, granted, pending_entry,
                pending_entry_free, pending_entry_actor_id,
                pending_entry_trigger, resume_parent_run_id, waited,
                created_at, updated_at)
               VALUES (%s, %s, false, 0, true, NULL, NULL, '', 0, false,
                       false, NULL, '', %s, 0, %s, %s)""",
            (ticker_id, issue_id, run_id, now, now),
        )
    return run_id, ticker_id, pod_id


def test_review_preview_escapes_non_ascii_parent_payload(api, seed, tenant_a,
                                                         project_a, write_a):
    ws = tenant_a["workspace"]["id"]
    state_id = seed.state(project_a, ws)
    issue_id = seed.issue(ws, project_a, name="Escaped payload",
                          state_id=state_id)
    run_id, ticker_id, pod_id = _seed_parent_run(
        seed, ws, project_a, issue_id, tenant_a["user"]["id"],
        {"summary": "fix \u2014 done \u2713", "note": "ship \U0001f680"},
    )
    try:
        r = api.post(_preview_url(tenant_a), headers=write_a,
                     json={"issue_id": str(issue_id)})
        assert r.status_code == 200, r.text
        prompt = r.json()["prompt"]
        assert '"summary": "fix \\u2014 done \\u2713"' in prompt
        assert '"note": "ship \\ud83d\\ude80"' in prompt
        assert "fix \u2014 done \u2713" not in prompt
        assert "ship \U0001f680" not in prompt
    finally:
        with seed.conn.cursor() as cur:
            cur.execute("DELETE FROM issue_agent_ticker WHERE id = %s",
                        (ticker_id,))
            cur.execute("DELETE FROM agent_run WHERE id = %s", (run_id,))
            cur.execute("DELETE FROM pod WHERE id = %s", (pod_id,))
