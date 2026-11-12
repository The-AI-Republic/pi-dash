"""Entry-run prompt sees no labels/assignees on create into a trigger state (PIDASHCONV-1368).

Django fires the orchestration state transition from ``post_save`` inside
``Issue.objects.create`` — before ``IssueCreateSerializer.create``
bulk-creates the ``IssueAssignee`` / ``IssueLabel`` rows — so the entry
run's first-turn prompt renders ``Labels: (none)`` / ``Assignees: (none)``
even when the request carried ``label_ids`` / ``assignee_ids``.

Seeds its own ticking state + pod + runner + label and cleans them up, so
the shared session seed is untouched.
"""

import uuid
from datetime import datetime, timezone

NOW = datetime.now(timezone.utc)

LABEL_NAME = "CT1368 Entry"


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def _seed_entry_world(cur, seed):
    """Insert In-Progress state, default pod, creator-owned runner, label."""
    ws, admin, project = seed["ws"], seed["admin"], seed["project"]
    prog, pod, runner, label = (str(uuid.uuid4()) for _ in range(4))
    # Idempotency: an interrupted run may have left CT1368 rows behind, and
    # the default-pod pick is ORDER BY created_at — stale rows would win.
    cur.execute("DELETE FROM pod WHERE project_id = %s AND name = 'is-entry-pod'", (project,))
    cur.execute("DELETE FROM states WHERE project_id = %s AND name = 'In Progress'", (project,))
    cur.execute("DELETE FROM labels WHERE project_id = %s AND name = %s", (project, LABEL_NAME))
    cur.execute(
        "INSERT INTO states (created_at, updated_at, id, name, description,"
        " color, slug, project_id, workspace_id, sequence, \"group\","
        " \"default\", is_triage)"
        " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
        # Literal "In Progress": the trigger registry pins exact state names.
        (NOW, NOW, prog, "In Progress", "", "#000000",
         "in-progress", project, ws, 0.0, "started", False, False),
    )
    cur.execute(
        "INSERT INTO pod (id, name, description, is_default,"
        " workspace_id, project_id, created_at, updated_at)"
        " VALUES (%s,%s,'',%s,%s,%s,%s,%s)",
        (pod, "is-entry-pod", True, ws, project, NOW, NOW),
    )
    cur.execute(
        """INSERT INTO runner
           (id, owner_id, workspace_id, pod_id, name, host_label,
            provisioning, visibility, status, capabilities,
            os, arch, runner_version,
            refresh_token_hash, refresh_token_fingerprint,
            refresh_token_generation, previous_refresh_token_hash,
            access_token_signing_key_version,
            enrollment_token_hash, enrollment_token_fingerprint,
            enrolled_at, revoked_at, revoked_reason,
            dev_machine_id, dev_metadata, protocol_version,
            created_at, updated_at)
           VALUES (%s,%s,%s,%s,'is-entry-runner','', 'manual',0,'online','[]',
                   '','','',
                   '','',0,'',1,
                   '','',
                   NULL,NULL,'',
                   NULL,'{}',1,
                   %s,%s)""",
        (runner, admin, ws, pod, NOW, NOW),
    )
    cur.execute(
        "INSERT INTO labels (created_at, updated_at, id, name,"
        " description, created_by_id, project_id, updated_by_id,"
        " workspace_id, color, sort_order)"
        " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
        (NOW, NOW, label, LABEL_NAME, "", admin, project, admin, ws,
         "#ff0000", 0.0),
    )
    cur.execute(
        "SELECT default_agent_executor FROM projects WHERE id = %s", (project,))
    prior_executor = cur.fetchone()[0]
    cur.execute(
        "UPDATE projects SET default_agent_executor = 'local_runner'"
        " WHERE id = %s", (project,))
    return {"prog": prog, "pod": pod, "runner": runner, "label": label,
            "prior_executor": prior_executor}


def _cleanup_entry_world(cur, seed, world, issue_id):
    if issue_id is not None:
        cur.execute("DELETE FROM agent_run WHERE work_item_id = %s", (issue_id,))
        cur.execute(
            "DELETE FROM issue_agent_ticker WHERE issue_id = %s", (issue_id,))
        cur.execute(
            "DELETE FROM issue_comments WHERE issue_id = %s", (issue_id,))
        cur.execute(
            "DELETE FROM issue_sequences WHERE issue_id = %s", (issue_id,))
        cur.execute(
            "DELETE FROM issue_assignees WHERE issue_id = %s", (issue_id,))
        cur.execute(
            "DELETE FROM issue_labels WHERE issue_id = %s", (issue_id,))
        cur.execute("DELETE FROM issues WHERE id = %s", (issue_id,))
    cur.execute("DELETE FROM runner WHERE id = %s", (world["runner"],))
    cur.execute("DELETE FROM pod WHERE id = %s", (world["pod"],))
    cur.execute("DELETE FROM labels WHERE id = %s", (world["label"],))
    cur.execute("DELETE FROM states WHERE id = %s", (world["prog"],))
    cur.execute(
        "UPDATE projects SET default_agent_executor = %s WHERE id = %s",
        (world["prior_executor"], seed["project"]),
    )


def test_create_into_trigger_renders_empty_m2m_prompt(clients, seed, db_conn):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    with db_conn.cursor() as cur:
        world = _seed_entry_world(cur, seed)
    issue_id = None
    try:
        resp = admin.post(
            f"{_base(ws, pid)}/issues/",
            json={"name": "CT1368 entry", "state_id": world["prog"],
                  "label_ids": [world["label"]],
                  "assignee_ids": [seed["member"]]},
        )
        assert resp.status_code == 201, resp.text
        body = resp.json()
        issue_id = body["id"]
        # The m2m legs landed (the request data is on the issue) ...
        assert body["state_id"] == world["prog"]
        assert body["label_ids"] == [world["label"]]
        assert body["assignee_ids"] == [seed["member"]]
        with db_conn.cursor() as cur:
            cur.execute(
                "SELECT trigger, prompt FROM agent_run"
                " WHERE work_item_id = %s ORDER BY created_at",
                (issue_id,),
            )
            runs = cur.fetchall()
            assert len(runs) == 1
            assert runs[0][0] == "state_transition"
            prompt = runs[0][1] or ""
            # ... but the entry prompt was rendered before they were written.
            assert "- Labels: (none)" in prompt
            assert "- Assignees: (none)" in prompt
            assert LABEL_NAME not in prompt
    finally:
        with db_conn.cursor() as cur:
            _cleanup_entry_world(cur, seed, world, issue_id)
