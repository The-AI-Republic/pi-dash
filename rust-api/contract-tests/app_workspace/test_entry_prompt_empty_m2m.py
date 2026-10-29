"""Entry-run prompt sees no labels/assignees on draft-to-issue into a trigger state (PIDASHCONV-1368).

Same ordering as issue create: Django renders the entry run's first-turn
prompt from ``post_save`` inside ``Issue.objects.create``, before the
``IssueAssignee`` / ``IssueLabel`` bulk-creates, so the prompt carries
``Labels: (none)`` / ``Assignees: (none)`` even when the convert request
carried ``label_ids`` / ``assignee_ids``.

The suite wipe does not cover pod/runner/agent_run/ticker/comments/labels,
so this test deletes its own rows in a finally.
"""

import uuid

LABEL_NAME = "CT1368 Entry"


def _seed_entry_world(db, ws, proj, creator_id, mate_id):
    prog, pod, runner, label = (str(uuid.uuid4()) for _ in range(4))
    with db.connect() as conn:
        with conn.cursor() as cur:
            for uid, role in ((creator_id, 20), (mate_id, 15)):
                cur.execute(
                    """INSERT INTO project_members (id, role, member_id,
                       project_id, workspace_id, view_props, default_props,
                       sort_order, preferences, is_active, created_at,
                       updated_at)
                       VALUES (%s,%s,%s,%s,%s,'{}','{}',0,'{}',true,now(),now())""",
                    (str(uuid.uuid4()), role, uid, proj["id"], ws["id"]),
                )
            for sid, name, group, default in (
                (str(uuid.uuid4()), "Backlog", "backlog", True),
                # Literal "In Progress": the trigger registry pins exact names.
                (prog, "In Progress", "started", False),
            ):
                cur.execute(
                    """INSERT INTO states (id, name, description, color, slug,
                       project_id, workspace_id, sequence, "group", "default",
                       is_triage, created_at, updated_at)
                       VALUES (%s,%s,'','#808080',%s,%s,%s,1.0,%s,%s,false,now(),now())""",
                    (sid, name, name.lower().replace(" ", "-"), proj["id"],
                     ws["id"], group, default),
                )
            cur.execute(
                """INSERT INTO labels (created_at, updated_at, id, name,
                   description, created_by_id, project_id, updated_by_id,
                   workspace_id, color, sort_order)
                   VALUES (now(),now(),%s,%s,'',%s,%s,%s,%s,'#ff0000',0.0)""",
                (label, LABEL_NAME, creator_id, proj["id"], creator_id,
                 ws["id"]),
            )
            cur.execute(
                """INSERT INTO pod (id, name, description, is_default,
                   workspace_id, project_id, created_at, updated_at)
                   VALUES (%s,'is-entry-pod','',true,%s,%s,now(),now())""",
                (pod, ws["id"], proj["id"]),
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
                           now(),now())""",
                (runner, creator_id, ws["id"], pod),
            )
        conn.commit()
    return {"prog": prog, "pod": pod, "runner": runner, "label": label}


def _cleanup_entry_world(db, world, issue_id):
    with db.connect() as conn:
        with conn.cursor() as cur:
            if issue_id is not None:
                # Issue graph first: the issue row pins the pod
                # (issues.assigned_pod_id) and the run.
                cur.execute(
                    "DELETE FROM agent_run WHERE work_item_id = %s", (issue_id,))
                cur.execute(
                    "DELETE FROM issue_agent_ticker WHERE issue_id = %s",
                    (issue_id,),
                )
                cur.execute(
                    "DELETE FROM issue_comments WHERE issue_id = %s", (issue_id,))
                cur.execute(
                    "DELETE FROM issue_sequences WHERE issue_id = %s",
                    (issue_id,),
                )
                cur.execute(
                    "DELETE FROM issue_assignees WHERE issue_id = %s",
                    (issue_id,),
                )
                cur.execute(
                    "DELETE FROM issue_labels WHERE issue_id = %s", (issue_id,))
                cur.execute("DELETE FROM issues WHERE id = %s", (issue_id,))
            cur.execute("DELETE FROM runner WHERE id = %s", (world["runner"],))
            cur.execute("DELETE FROM pod WHERE id = %s", (world["pod"],))
            cur.execute("DELETE FROM labels WHERE id = %s", (world["label"],))
        conn.commit()


def test_draft_to_issue_into_trigger_renders_empty_m2m_prompt(
    user_api, world
):
    db = world["db"]
    ws = db.make_workspace("Acme", f"acme-{uuid.uuid4().hex[:6]}",
                           world["user"]["id"])
    db.make_workspace_member(ws["id"], world["user"]["id"], role=20)
    mate = db.make_user(f"mate-{uuid.uuid4().hex[:6]}@example.com",
                        first_name="Asa", last_name="Signee")
    db.make_workspace_member(ws["id"], mate["id"], role=15)
    proj = db.make_project(ws["id"], f"E{uuid.uuid4().hex[:5].upper()}")
    world_seed = _seed_entry_world(db, ws, proj, world["user"]["id"],
                                   mate["id"])
    issue_id = None
    try:
        did = user_api.post(
            f"/api/workspaces/{ws['slug']}/draft-issues/",
            json={"name": "CT1368 draft", "project_id": proj["id"]},
        ).json()["id"]
        res = user_api.post(
            f"/api/workspaces/{ws['slug']}/draft-to-issue/{did}/",
            json={"name": "CT1368 from draft",
                  "state_id": world_seed["prog"],
                  "label_ids": [world_seed["label"]],
                  "assignee_ids": [mate["id"]]},
        )
        assert res.status_code == 201, res.text
        body = res.json()
        issue_id = body["id"]
        # The m2m legs landed (the request data is on the issue) ...
        assert body["state_id"] == world_seed["prog"]
        assert body["label_ids"] == [world_seed["label"]]
        assert body["assignee_ids"] == [mate["id"]]
        with db.connect() as conn:
            with conn.cursor() as cur:
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
        _cleanup_entry_world(db, world_seed, issue_id)
