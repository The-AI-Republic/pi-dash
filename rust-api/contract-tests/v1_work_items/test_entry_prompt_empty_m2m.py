"""Entry-run prompt sees no labels/assignees on v1 create into a trigger state (PIDASHCONV-1507).

Same ordering bug as PIDASHCONV-1368, on the v1 work-item create path:
Django fires the orchestration state transition from ``post_save`` inside
``Issue.objects.create`` — before the v1 serializer bulk-creates the
``IssueAssignee`` / ``IssueLabel`` rows — so the entry run's first-turn
prompt renders ``Labels: (none)`` / ``Assignees: (none)`` even when the
request carried ``labels`` / ``assignees``.

Seeds its own ticking state + default pod + runner + label via the
function-scoped ``seeder`` (tracked for teardown) and deletes the
server-created rows in a finally, so the shared world is untouched.
"""

import uuid
from datetime import datetime, timezone

import pytest

from .conftest import issue_url

pytestmark = pytest.mark.contract

NOW = datetime.now(timezone.utc)

LABEL_NAME = "CT1507 Entry"


def _seed_runner(db, seeder, owner_id, ws_id, pod_id):
    """Creator-owned runner row (no seeder factory exists); tracked for teardown."""
    runner_id = str(uuid.uuid4())
    db.execute(
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
        (runner_id, owner_id, ws_id, pod_id, NOW, NOW),
    )
    seeder.track.add("runner", runner_id)
    return runner_id


def test_create_into_trigger_renders_empty_m2m_prompt(api, world, seeder, db):
    ws_id = world["workspace"]["id"]
    project_id = world["project"]["id"]
    owner_id = world["owner"]["id"]
    # Literal "In Progress": the trigger registry pins exact state names.
    prog = seeder.create_state(ws_id, project_id, name="In Progress", group="started")
    pod = seeder.create_pod(ws_id, project_id, name="is-entry-pod", is_default=True)
    label = seeder.create_label(ws_id, project_id, name=LABEL_NAME)
    _seed_runner(db, seeder, owner_id, ws_id, pod["id"])
    prior_executor = db.fetchval(
        "SELECT default_agent_executor FROM projects WHERE id = %s", (project_id,)
    )
    db.execute(
        "UPDATE projects SET default_agent_executor = 'local_runner' WHERE id = %s",
        (project_id,),
    )
    issue_id = None
    try:
        resp = api.post(
            issue_url(world),
            json={
                "name": "CT1507 entry",
                "state": prog["id"],
                "labels": [label["id"]],
                "assignees": [owner_id],
            },
        )
        assert resp.status_code == 201, resp.text
        body = resp.json()
        issue_id = body["id"]
        # The m2m legs landed (the request data is on the issue) ...
        assert body["state"] == prog["id"]
        assert body["labels"] == [label["id"]]
        assert body["assignees"] == [owner_id]
        runs = db.fetchall(
            "SELECT trigger, prompt FROM agent_run"
            " WHERE work_item_id = %s ORDER BY created_at",
            (issue_id,),
        )
        assert len(runs) == 1
        assert runs[0]["trigger"] == "state_transition"
        prompt = runs[0]["prompt"] or ""
        # ... but the entry prompt was rendered before they were written.
        assert "- Labels: (none)" in prompt
        assert "- Assignees: (none)" in prompt
        assert LABEL_NAME not in prompt
    finally:
        # Server-created side rows are untracked: remove them so teardown
        # leaves no orphans behind.
        db.execute(
            "UPDATE projects SET default_agent_executor = %s WHERE id = %s",
            (prior_executor, project_id),
        )
        if issue_id is not None:
            db.execute("DELETE FROM agent_run WHERE work_item_id = %s", (issue_id,))
            db.execute("DELETE FROM issue_agent_ticker WHERE issue_id = %s", (issue_id,))
            db.execute("DELETE FROM issue_comments WHERE issue_id = %s", (issue_id,))
            db.execute("DELETE FROM issue_sequences WHERE issue_id = %s", (issue_id,))
            db.execute("DELETE FROM issue_assignees WHERE issue_id = %s", (issue_id,))
            db.execute("DELETE FROM issue_labels WHERE issue_id = %s", (issue_id,))
            db.execute("DELETE FROM issues WHERE id = %s", (issue_id,))
