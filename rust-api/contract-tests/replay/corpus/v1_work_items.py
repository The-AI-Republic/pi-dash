# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: api-v1 work-item write flows (PIDASHCONV-823).

Built from the ``v1_work_items`` contract suite (PIDASHCONV-76):
test_work_items.py (CRUD), test_subresources.py (links, comments,
relations, workpad, PR links), test_labels_pages.py (labels, pages),
test_actions.py (move, re-tick, wait, run-ai). The follow-up session
below covers the legs the lifecycle session skipped: the deprecated
``issues/*`` twins, work-item attachments, code-review links, the base
relations POST, intake issues, page patch/archive, and the agent-run
yield.
"""

from psycopg.rows import dict_row

from ..session import Request, Session

# Fixed move-target project id (seeded by _seed_move_target, addressed
# as {target_project} in request templates).
TARGET_PROJECT_ID = "aaaaaaaa-0000-4000-8000-000000000001"

# Fixed ids for the follow-up session (anchor issue, assets, page,
# triage state, intake, pod + agent run).
ISS_B_ID = "ffffffff-0000-4000-8000-000000000001"
ISS_C_ID = "ffffffff-0000-4000-8000-00000000000b"
ATT_WI_ID = "ffffffff-0000-4000-8000-000000000002"
ATT_IS_ID = "ffffffff-0000-4000-8000-000000000003"
PAGE_ID = "ffffffff-0000-4000-8000-000000000004"
TRIAGE_ID = "ffffffff-0000-4000-8000-000000000005"
INTAKE_ID = "ffffffff-0000-4000-8000-000000000006"
POD_ID = "ffffffff-0000-4000-8000-000000000007"
RUN_ID = "ffffffff-0000-4000-8000-000000000008"
RUN2_ID = "ffffffff-0000-4000-8000-000000000009"

_WS = "/api/v1/workspaces/{ws}/projects/{project}"
_WI = _WS + "/work-items/{wi}/"
_WI_NEW = _WS + "/work-items/{newwi}/"
_IS = "/api/v1/workspaces/{ws}/projects/{project}/issues"
_WB = "/api/v1/workspaces/{ws}/projects/{project}/work-items/{is_b}"


def _seed_move_target(conn) -> None:
    """Seed a second project in the world workspace (fixed id).

    Column list mirrors ``_harness.db.create_project`` (same values);
    only the id is fixed so requests can address it.
    """
    from _harness import db as db_helpers

    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = cur.fetchone()["id"]
        cur.execute(
            "SELECT id FROM users WHERE email = 'replay-admin@example.com'"
        )
        admin_id = cur.fetchone()["id"]
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO projects (id, name, description, network, identifier,
                workspace_id, created_by_id, cycle_view, module_view,
                issue_views_view, page_view, intake_view, archive_in, close_in,
                logo_props, is_time_tracking_enabled, is_issue_type_enabled,
                guest_view_all_features, timezone, members_can_edit_states,
                repo_url, base_branch, agent_default_interval_seconds,
                agent_default_max_ticks, agent_ticking_enabled, is_default,
                agent_review_default_interval_seconds, default_agent_executor,
                agent_test_default_interval_seconds, created_at, updated_at)
            VALUES (%s, 'Replay Move Target', '', 0, 'RPJMOVE',
                %s, %s, true, true,
                true, true, false, 0, 0,
                '{}', true, true,
                false, 'UTC', false,
                '', '', 0,
                0, false, false,
                0, '', 0, now(), now())
            """,
            (TARGET_PROJECT_ID, str(ws_id), str(admin_id)),
        )
        cur.execute(
            """
            INSERT INTO project_identifiers (name, project_id, workspace_id,
                created_by_id, created_at, updated_at)
            VALUES ('RPJMOVE', %s, %s, %s, now(), now())
            """,
            (TARGET_PROJECT_ID, str(ws_id), str(admin_id)),
        )
    conn.commit()
    db_helpers.add_project_member(conn, str(ws_id), TARGET_PROJECT_ID, str(admin_id), db_helpers.ADMIN)
    db_helpers.create_state(
        conn, str(ws_id), TARGET_PROJECT_ID, "replay-move",
        name="Move Backlog", group="backlog", default=True,
        created_by_id=str(admin_id),
    )


PR_PAYLOAD = {
    "repo_owner": "acme",
    "repo_name": "web",
    "pr_number": 424242,
    "url": "https://github.com/acme/web/pull/424242",
    "title": "Fix it",
    "state": "open",
    "merged": False,
    "draft": False,
}

CR_PAYLOAD = {
    "provider": "github",
    "host_url": "https://github.com",
    "namespace": "acme",
    "repo_name": "web",
    "repo_external_id": "",
    "external_id": "mr-424243",
    "external_iid": "424243",
    "url": "https://github.com/acme/web/pull/424243",
    "title": "Review me",
    "state": "open",
    "merged": False,
    "draft": False,
}


def _seed_followup(conn) -> None:
    """Anchor rows the follow-up session addresses by fixed id.

    Mirrors the contract seeders (same columns): an anchor issue on
    RPJ's backlog (seq 906, clear of API-created sequences), two
    uploaded file assets (``_harness.seed.create_issue_attachment``
    columns), a page + project link (``create_page`` /
    ``link_page_to_project``), a triage state + intake row
    (``seed_d21`` columns — intake creation mints into triage), and a
    pod + running agent run (``create_pod`` / ``create_agent_run``)
    for the yield legs.
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM projects WHERE identifier = 'RPJ'")
        project_id = str(cur.fetchone()["id"])
        cur.execute(
            "SELECT id FROM states WHERE workspace_id = %s AND \"group\" = 'backlog'",
            (ws_id,),
        )
        backlog_id = str(cur.fetchone()["id"])
        cur.execute(
            "SELECT id FROM users WHERE email = 'replay-admin@example.com'"
        )
        admin_id = str(cur.fetchone()["id"])
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO issues (id, created_at, updated_at, name, description_json,
                priority, sequence_id, project_id, state_id,
                workspace_id, description_html, sort_order,
                is_draft, git_work_branch, workpad, complexity_score)
            VALUES (%s, now(), now(), 'Replay v1 anchor', '{}', 'high', 906,
                %s, %s, %s, '', 65535, false, '', '', 0)
            """,
            (ISS_B_ID, project_id, backlog_id, ws_id),
        )
        # Second anchor: agent_run_one_active_per_work_item allows one
        # running run per issue, and the missing-outcome 400 leg needs
        # its own fresh running run.
        cur.execute(
            """
            INSERT INTO issues (id, created_at, updated_at, name, description_json,
                priority, sequence_id, project_id, state_id,
                workspace_id, description_html, sort_order,
                is_draft, git_work_branch, workpad, complexity_score)
            VALUES (%s, now(), now(), 'Replay v1 anchor C', '{}', 'high', 907,
                %s, %s, %s, '', 65535, false, '', '', 0)
            """,
            (ISS_C_ID, project_id, backlog_id, ws_id),
        )
        for asset_id, asset in (
            (ATT_WI_ID, "replay-wi-asset"),
            (ATT_IS_ID, "replay-is-asset"),
        ):
            cur.execute(
                """
                INSERT INTO file_assets
                   (id, attributes, asset, entity_type, is_deleted, is_archived,
                    is_uploaded, issue_id, project_id, workspace_id, size,
                    created_at, updated_at)
                VALUES (%s,'{}',%s,'ISSUE_ATTACHMENT',false,false,true,%s,%s,%s,10,now(),now())
                """,
                (asset_id, asset, ISS_B_ID, project_id, ws_id),
            )
        cur.execute(
            """
            INSERT INTO pages
               (id, name, description_json, description_binary, description_html,
                owned_by_id, access, workspace_id, color, parent_id, archived_at,
                is_locked, view_props, logo_props, is_global, sort_order,
                created_by_id, created_at, updated_at)
            VALUES (%s,'Replay v1 page','{}',NULL,'<p></p>',%s,0,%s,'',NULL,NULL,
                    false,'{"full_width": false}','{}',false,65535,%s,now(),now())
            """,
            (PAGE_ID, admin_id, ws_id, admin_id),
        )
        cur.execute(
            """
            INSERT INTO project_pages
               (id, page_id, project_id, workspace_id, created_at, updated_at)
            VALUES (gen_random_uuid(),%s,%s,%s,now(),now())
            """,
            (PAGE_ID, project_id, ws_id),
        )
        cur.execute(
            """
            INSERT INTO states (created_at, updated_at, id, name, description,
                color, slug, project_id, workspace_id, sequence, "group",
                "default", is_triage)
            VALUES (now(), now(), %s, 'Triage', '', '#4E5355',
                'triage', %s, %s, 65000.0, 'triage', false, true)
            """,
            (TRIAGE_ID, project_id, ws_id),
        )
        cur.execute(
            """
            INSERT INTO intakes (id, name, description, is_default, view_props,
                logo_props, created_by_id, project_id, workspace_id, created_at, updated_at)
            VALUES (%s,'Replay intake','Seeded for write-replay',true,'{}','{}',%s,%s,%s,now(),now())
            """,
            (INTAKE_ID, admin_id, project_id, ws_id),
        )
        cur.execute(
            """
            INSERT INTO pod
               (id, name, description, is_default, workspace_id, project_id,
                created_at, updated_at)
            VALUES (%s,'Replay v1 pod','',false,%s,%s,now(),now())
            """,
            (POD_ID, ws_id, project_id),
        )
        cur.execute(
            """
            INSERT INTO agent_run
               (id, status, prompt, run_config, required_capabilities, thread_id,
                error, workspace_id, work_item_id, pod_id, created_by_id, llm_model,
                refusal_category, trigger, executor_kind, dispatch_attempts,
                cancel_reason, error_code, tool_plan, phase_kind, agent_metadata,
                usage, created_at, work_type)
            VALUES (%s,'running','contract prompt','{}','{}','contract-thread','',
                    %s,%s,%s,%s,'contract-model','', 'human', 'local_runner', 0,
                    '','', '{}', 'work', '{}', '{"input": 0, "output": 0, "total": 0}', now(),'')
            """,
            (RUN_ID, ws_id, ISS_B_ID, POD_ID, admin_id),
        )
        # Second running run: the missing-outcome 400 leg runs against
        # a fresh run exactly like the suite (the first run is done by
        # then, and yield-on-done may answer differently).
        cur.execute(
            """
            INSERT INTO agent_run
               (id, status, prompt, run_config, required_capabilities, thread_id,
                error, workspace_id, work_item_id, pod_id, created_by_id, llm_model,
                refusal_category, trigger, executor_kind, dispatch_attempts,
                cancel_reason, error_code, tool_plan, phase_kind, agent_metadata,
                usage, created_at, work_type)
            VALUES (%s,'running','contract prompt','{}','{}','contract-thread','',
                    %s,%s,%s,%s,'contract-model','', 'human', 'local_runner', 0,
                    '','', '{}', 'work', '{}', '{"input": 0, "output": 0, "total": 0}', now(),'')
            """,
            (RUN2_ID, ws_id, ISS_C_ID, POD_ID, admin_id),
        )
    conn.commit()

SESSIONS = (
    Session(
        name="v1_work_items_lifecycle",
        domain="v1_work_items",
        source="v1_work_items/test_work_items.py, test_subresources.py, "
        "test_labels_pages.py, test_actions.py (PIDASHCONV-76)",
        auth_users=("api_key",),
        seed=_seed_move_target,
        static_context={"target_project": TARGET_PROJECT_ID},
        # process_logs embeds two timezone.now() stamps per backend
        # (Django micros, Rust seconds): each side mints its own now,
        # so both blank via the explicit time map.
        payload_time_paths={
            "pi_dash.bgtasks.issue_activities_task.issue_activity": ["kwargs/epoch"],
            "pi_dash.bgtasks.logger_task.process_logs": [
                "kwargs/mongo_log/created_at",
                "kwargs/mongo_log/updated_at",
            ],
        },
        # process_logs now compares exactly (868/920/937 all landed); only the now() stamps above still blank.
        # soft_delete_related_objects("db", "<model>", <row id>): the row
        # id is backend-local (never in a response); each side's value
        # must exist in its own table.
        payload_row_refs={
            "pi_dash.bgtasks.deletion_task.soft_delete_related_objects": [
                ("args/2", "issue_relations"),
                ("args/2", "git_code_review_links"),
                ("args/2", "issues"),
                ("args/2", "issue_comments"),
                ("args/2", "issue_links"),
                ("args/2", "labels"),
            ],
        },
        # PR-link create also writes the legacy link row and stashes its
        # id in the new row's metadata (github_pr_links.py:_ensure...).
        db_cell_refs=[
            ("git_code_review_links", "metadata", "github_pull_request_links",
             "legacy_github_pull_request_link_id"),
        ],
        requests=(
            Request(
                "POST", _WS + "/work-items/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/",
                json={"name": "Replay v1", "description_html": "<p>body</p>", "priority": "high"},
                auth="api_key", capture={"wi": "id"}, expect_status=201,
            ),
            Request(
                "PATCH", _WI,
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/",
                json={"name": "Replay v1b", "priority": "low"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _WS + "/work-items/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/",
                json={"name": "Sibling"},
                auth="api_key", capture={"sib": "id"}, expect_status=201,
            ),
            Request(
                "POST", _WI + "relations/relate/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/relations/relate/",
                json={"relation_type": "relates_to", "issues": ["{sib}"]},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _WI + "relations/unrelate/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/relations/unrelate/",
                json={"relation_type": "relates_to", "issues": ["{sib}"]},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _WI + "comments/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/comments/",
                json={"comment_html": "<p>hi</p>", "access": "EXTERNAL"},
                auth="api_key", capture={"wcomment": "id", "wcomment_desc": "description"},
                expect_status=201,
            ),
            Request(
                "PATCH", _WI + "comments/{wcomment}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/comments/{comment_id}/",
                json={"comment_html": "<p>v2</p>"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _WI + "links/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/links/",
                json={"title": "Spec", "url": "https://example.com/spec"},
                auth="api_key", capture={"link": "id"}, expect_status=201,
            ),
            Request(
                "PATCH", _WI + "links/{link}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/links/{link_id}/",
                json={"title": "New"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "PATCH", _WI + "workpad/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/workpad/",
                json={"body": "# notes"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _WI + "github/pull-requests/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/github/pull-requests/",
                json=dict(PR_PAYLOAD),
                auth="api_key", capture={"pr": "id"}, expect_status=201,
            ),
            Request(
                "POST", _WI + "github/pull-requests/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/github/pull-requests/",
                json=dict(PR_PAYLOAD),
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _WI + "move/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/move/",
                json={},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", _WI + "wait/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/wait/",
                json={},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _WI + "re-tick/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/re-tick/",
                json={},
                auth="api_key", capture={"tick_run": "run_id"}, expect_status=200,
            ),
            Request(
                "POST", _WI + "run-ai/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/run-ai/",
                json={"prompt": "do it"},
                auth="api_key", expect_status=409,
            ),
            Request(
                "POST", _WS + "/labels/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/labels/",
                json={"name": "Bug", "color": "#ff0000", "description": "defects"},
                auth="api_key", capture={"label": "id"}, expect_status=201,
            ),
            Request(
                "PATCH", _WS + "/labels/{label}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/labels/{pk}/",
                json={"name": "New"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _WS + "/pages/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/pages/",
                json={"name": "Nope", "description_markdown": "# hi"},
                auth="api_key", expect_status=503,
            ),
            Request(
                "DELETE", _WI + "github/pull-requests/{pr}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/github/pull-requests/{link_id}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _WI + "links/{link}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/links/{link_id}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _WI + "comments/{wcomment}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/comments/{comment_id}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "POST", _WI + "move/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/move/",
                json={"project": "{target_project}"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "DELETE", _WS + "/labels/{label}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/labels/{pk}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _WS + "/work-items/{sib}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE",
                "/api/v1/workspaces/{ws}/projects/{target_project}/work-items/{wi}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/",
                auth="api_key", expect_status=204,
            ),
        ),
    ),
    Session(
        name="v1_work_items_followup",
        domain="v1_work_items",
        source="v1_work_items/test_work_items.py (issues twins), "
        "test_subresources.py (attachments, code-reviews, relations), "
        "test_labels_pages.py (pages), test_actions.py (yield), "
        "api_assets_stickies_intake/test_intake.py (intake)",
        auth_users=("api_key",),
        seed=_seed_followup,
        static_context={
            "is_b": ISS_B_ID,
            "att_wi": ATT_WI_ID,
            "att_is": ATT_IS_ID,
            "pg1": PAGE_ID,
            "triage": TRIAGE_ID,
            "intake": INTAKE_ID,
            "pod1": POD_ID,
            "run1": RUN_ID,
            "run2": RUN2_ID,
        },
        # process_logs embeds two timezone.now() stamps per backend
        # (Django micros, Rust seconds): each side mints its own now,
        # so both blank via the explicit time map.
        payload_time_paths={
            "pi_dash.bgtasks.issue_activities_task.issue_activity": ["kwargs/epoch"],
            "pi_dash.bgtasks.logger_task.process_logs": [
                "kwargs/mongo_log/created_at",
                "kwargs/mongo_log/updated_at",
            ],
        },
        payload_row_refs={
            "pi_dash.bgtasks.deletion_task.soft_delete_related_objects": [
                ("args/2", "issues"),
                ("args/2", "issue_comments"),
                ("args/2", "issue_links"),
                ("args/2", "issue_relations"),
                ("args/2", "file_assets"),
                ("args/2", "git_code_review_links"),
                ("args/2", "github_pull_request_links"),
                ("args/2", "intake_issues"),
            ],
        },
        # process_logs now compares exactly (868/920/937 all landed); only the now() stamps above still blank.
        requests=(
            # issues/* twins: create -> patch -> comment round-trip ->
            # attachment 400-POST + patch + delete -> link round-trip
            # -> delete.
            Request(
                "POST", _IS + "/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/issues/",
                json={"name": "Replay twin"},
                auth="api_key", capture={"is_a": "id"}, expect_status=201,
            ),
            Request(
                "PATCH", _IS + "/{is_a}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
                json={"name": "Replay twin v2"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _IS + "/{is_b}/comments/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/comments/",
                json={"comment_html": "<p>twin hello</p>", "access": "EXTERNAL"},
                auth="api_key",
                capture={"isc": "id", "isc_desc": "description"},
                expect_status=201,
            ),
            Request(
                "PATCH", _IS + "/{is_b}/comments/{isc}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/comments/{comment_id}/",
                json={"comment_html": "<p>twin v2</p>"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "DELETE", _IS + "/{is_b}/comments/{isc}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/comments/{comment_id}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "POST", _IS + "/{is_b}/issue-attachments/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/issue-attachments/",
                json={"name": "x.exe", "type": "application/x-msdownload", "size": 10},
                auth="api_key", expect_status=400,
            ),
            Request(
                "PATCH", _IS + "/{is_b}/issue-attachments/{att_is}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/issue-attachments/{attachment_id}/",
                json={},
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _IS + "/{is_b}/issue-attachments/{att_is}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/issue-attachments/{attachment_id}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "POST", _IS + "/{is_b}/links/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/links/",
                json={"title": "twin docs", "url": "https://example.com/twin"},
                auth="api_key", capture={"islink": "id"}, expect_status=201,
            ),
            Request(
                "PATCH", _IS + "/{is_b}/links/{islink}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/links/{link_id}/",
                json={"title": "twin docs v2"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "DELETE", _IS + "/{is_b}/links/{islink}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/links/{link_id}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _IS + "/{is_a}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
                auth="api_key", expect_status=204,
            ),
            # Base relations POST (the grouped relate twin): is_b
            # relates to a live sibling (created + deleted around it).
            Request(
                "POST", _IS + "/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/issues/",
                json={"name": "Replay rel sib"},
                auth="api_key", capture={"is_sib": "id"}, expect_status=201,
            ),
            Request(
                "POST", _WB + "/relations/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/relations/",
                json={"relation_type": "relates_to", "issues": ["{is_sib}"]},
                auth="api_key", expect_status=201,
            ),
            Request(
                "DELETE", _IS + "/{is_sib}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
                auth="api_key", expect_status=204,
            ),
            # Work-item attachments: 400-POST covers the create pair
            # (valid types need a bucket to presign against), PATCH on
            # the seeded row, DELETE soft-deletes.
            Request(
                "POST", _WB + "/attachments/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/attachments/",
                json={"name": "x.exe", "type": "application/x-msdownload", "size": 10},
                auth="api_key", expect_status=400,
            ),
            Request(
                "PATCH", _WB + "/attachments/{att_wi}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/attachments/{attachment_id}/",
                json={},
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _WB + "/attachments/{att_wi}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/attachments/{attachment_id}/",
                auth="api_key", expect_status=204,
            ),
            # Code-review links: attach 201, idempotent relink 200,
            # delete.
            Request(
                "POST", _WB + "/code-reviews/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/code-reviews/",
                json=CR_PAYLOAD,
                auth="api_key", capture={"cr": "id"}, expect_status=201,
            ),
            Request(
                "POST", _WB + "/code-reviews/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/code-reviews/",
                json=CR_PAYLOAD,
                auth="api_key", expect_status=200,
            ),
            Request(
                "DELETE", _WB + "/code-reviews/{cr}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/code-reviews/{review_id}/",
                auth="api_key", expect_status=204,
            ),
            # Pages: patch without a doc service is a 503 both sides;
            # archive + unarchive round-trip.
            Request(
                "PATCH", _WS + "/pages/{pg1}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/pages/{pk}/",
                json={"description_markdown": "# edit"},
                auth="api_key", expect_status=503,
            ),
            Request(
                "POST", _WS + "/pages/{pg1}/archive/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/pages/{pk}/archive/",
                auth="api_key", expect_status=200,
            ),
            Request(
                "DELETE", _WS + "/pages/{pg1}/archive/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/pages/{pk}/archive/",
                auth="api_key", expect_status=200,
            ),
            # Intake: create (mints into triage) -> missing-name 400
            # -> patch issue fields -> accept to the default backlog
            # -> delete the accepted item (issue survives) ->
            # create + delete a pending item (issue soft-deleted).
            Request(
                "POST", _WS + "/intake-issues/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/",
                json={"issue": {"name": "triage me", "priority": "high"}},
                auth="api_key",
                capture={"inta": "id", "inta_issue": "issue",
                         "inta_desc": "issue_detail/description"},
                expect_status=201,
            ),
            Request(
                "POST", _WS + "/intake-issues/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/",
                json={"issue": {"priority": "low"}},
                auth="api_key", expect_status=400,
            ),
            Request(
                "PATCH", _WS + "/intake-issues/{inta_issue}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/",
                json={"issue": {"name": "triaged", "priority": "urgent"}},
                auth="api_key", expect_status=200,
            ),
            Request(
                "PATCH", _WS + "/intake-issues/{inta_issue}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/",
                json={"status": 1},
                auth="api_key", expect_status=200,
            ),
            Request(
                "DELETE", _WS + "/intake-issues/{inta_issue}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "POST", _WS + "/intake-issues/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/",
                json={"issue": {"name": "doomed"}},
                auth="api_key",
                capture={"inta2": "id", "inta2_issue": "issue",
                         "inta2_desc": "issue_detail/description"},
                expect_status=201,
            ),
            Request(
                "DELETE", _WS + "/intake-issues/{inta2_issue}/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/",
                auth="api_key", expect_status=204,
            ),
            # Agent-run yield: happy path then missing-outcome 400.
            Request(
                "POST", "/api/v1/workspaces/{ws}/agent-runs/{run1}/yield/",
                route="/api/v1/workspaces/{slug}/agent-runs/{pk}/yield/",
                json={"outcome": "done", "note": "finished"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", "/api/v1/workspaces/{ws}/agent-runs/{run2}/yield/",
                route="/api/v1/workspaces/{slug}/agent-runs/{pk}/yield/",
                json={},
                auth="api_key", expect_status=400,
            ),
        ),
    ),
)
