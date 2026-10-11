# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: api-v1 cycle + module write flows (PIDASHCONV-823).

Built from the ``v1_cycles_modules`` contract suite (D-20): test_cycles.py
(cycles, cycle-issues, transfer, archive) and test_modules.py (modules,
module-issues, archive, date formats). API-key auth throughout, so the
868 process_logs pin applies.
"""

from datetime import datetime, timedelta, timezone

from psycopg.rows import dict_row

from ..session import Request, Session

# Fixed ids seeded by _seed_extras (addressed via static_context).
CY_ACTIVE_ID = "eeeeeeee-0000-4000-8000-000000000001"
CY_COMPLETED_ID = "eeeeeeee-0000-4000-8000-000000000002"
CY_DRAFT_ID = "eeeeeeee-0000-4000-8000-000000000003"
CY_ARCHIVED_ID = "eeeeeeee-0000-4000-8000-000000000004"
MOD_ACTIVE_ID = "eeeeeeee-0000-4000-8000-000000000005"
MOD_COMPLETED_ID = "eeeeeeee-0000-4000-8000-000000000006"
MOD_ARCHIVED_ID = "eeeeeeee-0000-4000-8000-000000000007"
ISS1_ID = "eeeeeeee-0000-4000-8000-000000000008"
ISS2_ID = "eeeeeeee-0000-4000-8000-000000000009"
ISS3_ID = "eeeeeeee-0000-4000-8000-00000000000a"

_V1 = "/api/v1/workspaces/{ws}/projects/{project}"


def _seed_extras(conn) -> None:
    """Cycles/modules/issues mirroring the D-20 ``db.reset`` seed.

    Same rows (active/completed/draft/archived cycles, active/
    completed/archived modules, open-state issues, bridges) on the
    replay world instead of the CT world; dates use now()-relative
    intervals exactly like the suite so the active/completed windows
    never rot. Bridge ids are random (the runner clones the Django
    copy into the Rust copy, and requests address bridges by issue).
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
        for iid, seq in (
            (ISS1_ID, 903), (ISS2_ID, 904), (ISS3_ID, 905),
        ):
            cur.execute(
                """
                INSERT INTO issues (id, created_at, updated_at, name, description_json,
                    priority, sequence_id, project_id, state_id,
                    workspace_id, description_html, sort_order,
                    is_draft, git_work_branch, workpad, complexity_score)
                VALUES (%s, now(), now(), %s, '{}', 'high', %s, %s, %s,
                    %s, '', 65535, false, '', '', 0)
                """,
                (iid, "Replay v1 issue %d" % seq, seq, project_id,
                 backlog_id, ws_id),
            )
        cur.execute(
            """
            INSERT INTO cycles (id, created_at, updated_at, name, description,
                start_date, end_date, owned_by_id, project_id, workspace_id,
                view_props, sort_order, progress_snapshot, archived_at,
                logo_props, timezone, version)
            VALUES
            (%s, now(), now(), 'Replay active cycle', '',
             now() - interval '1 day', now() + interval '6 days',
             %s, %s, %s, '{}', 65535, '{}', NULL, '{}', 'UTC', 1),
            (%s, now(), now(), 'Replay completed cycle', '',
             now() - interval '10 days', now() - interval '3 days',
             %s, %s, %s, '{}', 65535, '{}', NULL, '{}', 'UTC', 1),
            (%s, now(), now(), 'Replay draft cycle', '',
             NULL, NULL,
             %s, %s, %s, '{}', 65535, '{}', NULL, '{}', 'UTC', 1),
            (%s, now(), now(), 'Replay archived cycle', '',
             now() - interval '20 days', now() - interval '13 days',
             %s, %s, %s, '{}', 65535, '{}', now(), '{}', 'UTC', 1)
            """,
            (CY_ACTIVE_ID, admin_id, project_id, ws_id,
             CY_COMPLETED_ID, admin_id, project_id, ws_id,
             CY_DRAFT_ID, admin_id, project_id, ws_id,
             CY_ARCHIVED_ID, admin_id, project_id, ws_id),
        )
        for iid in (ISS1_ID, ISS3_ID):
            cur.execute(
                """
                INSERT INTO cycle_issues (id, created_at, updated_at, cycle_id,
                    issue_id, project_id, workspace_id)
                VALUES (gen_random_uuid(), now(), now(), %s, %s, %s, %s)
                """,
                (CY_ACTIVE_ID, iid, project_id, ws_id),
            )
        # Open-state issue linked to the completed cycle: the transfer
        # leg moves it (mirrors the suite's direct INSERT).
        cur.execute(
            """
            INSERT INTO cycle_issues (id, created_at, updated_at, cycle_id,
                issue_id, project_id, workspace_id)
            VALUES (gen_random_uuid(), now(), now(), %s, %s, %s, %s)
            """,
            (CY_COMPLETED_ID, ISS2_ID, project_id, ws_id),
        )
        cur.execute(
            """
            INSERT INTO modules (id, created_at, updated_at, name, description,
                status, project_id, workspace_id,
                view_props, sort_order, archived_at, logo_props)
            VALUES
            (%s, now(), now(), 'Replay active module', '',
             'planned', %s, %s, '{}', 65535, NULL, '{}'),
            (%s, now(), now(), 'Replay completed module', '',
             'completed', %s, %s, '{}', 65535, NULL, '{}'),
            (%s, now(), now(), 'Replay archived module', '',
             'completed', %s, %s, '{}', 65535, now(), '{}')
            """,
            (MOD_ACTIVE_ID, project_id, ws_id,
             MOD_COMPLETED_ID, project_id, ws_id,
             MOD_ARCHIVED_ID, project_id, ws_id),
        )
        cur.execute(
            """
            INSERT INTO module_issues (id, created_at, updated_at, issue_id,
                module_id, project_id, workspace_id)
            VALUES (gen_random_uuid(), now(), now(), %s, %s, %s, %s)
            """,
            (ISS1_ID, MOD_ACTIVE_ID, project_id, ws_id),
        )
    conn.commit()


_NOW = datetime.now(timezone.utc)
_DATED_START = (_NOW + timedelta(days=1)).strftime("%Y-%m-%dT%H:%M:%SZ")
_DATED_END = (_NOW + timedelta(days=8)).strftime("%Y-%m-%dT%H:%M:%SZ")

SESSIONS = (
    Session(
        name="v1_cycles_modules_lifecycle",
        domain="v1_cycles_modules",
        source="v1_cycles_modules/test_cycles.py, test_modules.py (D-20)",
        auth_users=("api_key",),
        seed=_seed_extras,
        static_context={
            "cy_active": CY_ACTIVE_ID,
            "cy_completed": CY_COMPLETED_ID,
            "cy_draft": CY_DRAFT_ID,
            "cy_archived": CY_ARCHIVED_ID,
            "mod_active": MOD_ACTIVE_ID,
            "mod_completed": MOD_COMPLETED_ID,
            "mod_archived": MOD_ARCHIVED_ID,
            "iss1": ISS1_ID,
            "iss2": ISS2_ID,
            "iss3": ISS3_ID,
        },
        # Known divergence: Rust never enqueues the API-token audit log
        # (PIDASHCONV-868, shared with the other v1 sessions).
        known_absent_jobs={
            "pi_dash.bgtasks.logger_task.process_logs": "PIDASHCONV-868",
        },
        # issue_activity carries int(timezone.now()) as `epoch`
        # (same window as the app_cycles session).
        payload_time_paths={
            "pi_dash.bgtasks.issue_activities_task.issue_activity": ["kwargs/epoch"],
        },
        requests=(
            # Cycles: minimal draft -> dated (future window, stays
            # deletable) -> empty-400 -> start-only-400 -> ext-id
            # create -> ext-id 409.
            Request(
                "POST", _V1 + "/cycles/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/",
                json={"name": "Replay V1C New"},
                auth="api_key", capture={"cy_new": "id"}, expect_status=201,
            ),
            Request(
                "POST", _V1 + "/cycles/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/",
                json={"name": "Replay V1C Dated",
                      "start_date": _DATED_START, "end_date": _DATED_END},
                auth="api_key", capture={"cy_dated": "id"}, expect_status=201,
            ),
            Request(
                "POST", _V1 + "/cycles/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/",
                json={},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", _V1 + "/cycles/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/",
                json={"name": "Replay V1C Half", "start_date": _DATED_START},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", _V1 + "/cycles/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/",
                json={"name": "Replay V1C Ext", "external_id": "rp-ext-1",
                      "external_source": "github"},
                auth="api_key", capture={"cy_ext": "id"}, expect_status=201,
            ),
            Request(
                "POST", _V1 + "/cycles/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/",
                json={"name": "Replay V1C Ext2", "external_id": "rp-ext-1",
                      "external_source": "github"},
                auth="api_key", expect_status=409,
            ),
            # Cycle detail: rename active -> completed 400 ->
            # sort_order no-op 200 (ported bug) -> archived 400 ->
            # ext-id setup on active -> ext-id 409 on draft.
            Request(
                "PATCH", _V1 + "/cycles/{cy_active}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/",
                json={"name": "Replay V1C Renamed"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "PATCH", _V1 + "/cycles/{cy_completed}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/",
                json={"name": "Replay V1C Nope"},
                auth="api_key", expect_status=400,
            ),
            Request(
                "PATCH", _V1 + "/cycles/{cy_completed}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/",
                json={"sort_order": 12.5},
                auth="api_key", expect_status=200,
            ),
            Request(
                "PATCH", _V1 + "/cycles/{cy_archived}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/",
                json={"name": "Replay V1C Nope2"},
                auth="api_key", expect_status=400,
            ),
            Request(
                "PATCH", _V1 + "/cycles/{cy_active}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/",
                json={"external_id": "rp-ext-9", "external_source": "github"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "PATCH", _V1 + "/cycles/{cy_draft}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/",
                json={"external_id": "rp-ext-9", "external_source": "github"},
                auth="api_key", expect_status=409,
            ),
            # Cycle deletes (the API-created rows).
            Request(
                "DELETE", _V1 + "/cycles/{cy_new}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _V1 + "/cycles/{cy_dated}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _V1 + "/cycles/{cy_ext}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/",
                auth="api_key", expect_status=204,
            ),
            # Cycle issues: add iss2 to active -> remove -> empty-400
            # -> completed-400.
            Request(
                "POST", _V1 + "/cycles/{cy_active}/cycle-issues/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/cycle-issues/",
                json={"issues": ["{iss2}"]},
                auth="api_key", expect_status=200,
            ),
            Request(
                "DELETE", _V1 + "/cycles/{cy_active}/cycle-issues/{iss2}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/cycle-issues/{iid}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "POST", _V1 + "/cycles/{cy_active}/cycle-issues/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/cycle-issues/",
                json={"issues": []},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", _V1 + "/cycles/{cy_completed}/cycle-issues/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/cycle-issues/",
                json={"issues": ["{iss1}"]},
                auth="api_key", expect_status=400,
            ),
            # Transfer: missing-target 400 -> incomplete-source 400 ->
            # success (iss2 moves completed -> active).
            Request(
                "POST", _V1 + "/cycles/{cy_completed}/transfer-issues/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/transfer-issues/",
                json={},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", _V1 + "/cycles/{cy_active}/transfer-issues/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/transfer-issues/",
                json={"new_cycle_id": "{cy_completed}"},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", _V1 + "/cycles/{cy_completed}/transfer-issues/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/transfer-issues/",
                json={"new_cycle_id": "{cy_active}"},
                auth="api_key", expect_status=200,
            ),
            # Archive: completed 204 -> unarchive 204 -> draft 500
            # (ported TypeError bug).
            Request(
                "POST", _V1 + "/cycles/{cy_completed}/archive/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/archive/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _V1 + "/archived-cycles/{cy_completed}/unarchive/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/archived-cycles/{cid}/unarchive/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "POST", _V1 + "/cycles/{cy_draft}/archive/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/cycles/{cid}/archive/",
                auth="api_key", expect_status=500,
            ),
            # Modules: minimal -> status+lead -> empty-400 ->
            # inverted-dates 400 -> dup-name 400 -> ext-id create ->
            # ext-id 409 -> week-date create.
            Request(
                "POST", _V1 + "/modules/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/",
                json={"name": "Replay V1M New"},
                auth="api_key", capture={"mod_new": "id"}, expect_status=201,
            ),
            Request(
                "POST", _V1 + "/modules/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/",
                json={"name": "Replay V1M Lead", "status": "in-progress",
                      "lead": "{admin}"},
                auth="api_key", capture={"mod_lead": "id"}, expect_status=201,
            ),
            Request(
                "POST", _V1 + "/modules/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/",
                json={},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", _V1 + "/modules/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/",
                json={"name": "Replay V1M Bad", "start_date": "2026-10-08",
                      "target_date": "2026-10-01"},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", _V1 + "/modules/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/",
                json={"name": "Replay V1M New"},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", _V1 + "/modules/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/",
                json={"name": "Replay V1M Ext", "external_id": "rp-mod-ext-1",
                      "external_source": "github"},
                auth="api_key", capture={"mod_ext": "id"}, expect_status=201,
            ),
            Request(
                "POST", _V1 + "/modules/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/",
                json={"name": "Replay V1M Ext2", "external_id": "rp-mod-ext-1",
                      "external_source": "github"},
                auth="api_key", expect_status=409,
            ),
            Request(
                "POST", _V1 + "/modules/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/",
                json={"name": "Replay V1M Week", "start_date": "2024-W05-6",
                      "target_date": "2024-W06-1"},
                auth="api_key", capture={"mod_week": "id"}, expect_status=201,
            ),
            # Module detail: rename active -> archived 400 -> ext-id
            # setup -> ext-id 409 on completed -> deletes.
            Request(
                "PATCH", _V1 + "/modules/{mod_active}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/{mid}/",
                json={"name": "Replay V1M Renamed"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "PATCH", _V1 + "/modules/{mod_archived}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/{mid}/",
                json={"name": "Replay V1M Nope"},
                auth="api_key", expect_status=400,
            ),
            Request(
                "PATCH", _V1 + "/modules/{mod_active}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/{mid}/",
                json={"external_id": "rp-mod-ext-9", "external_source": "github"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "PATCH", _V1 + "/modules/{mod_completed}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/{mid}/",
                json={"external_id": "rp-mod-ext-9", "external_source": "github"},
                auth="api_key", expect_status=409,
            ),
            Request(
                "DELETE", _V1 + "/modules/{mod_new}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/{mid}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _V1 + "/modules/{mod_lead}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/{mid}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _V1 + "/modules/{mod_ext}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/{mid}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _V1 + "/modules/{mod_week}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/{mid}/",
                auth="api_key", expect_status=204,
            ),
            # Module issues: add iss3 -> remove -> empty-400.
            Request(
                "POST", _V1 + "/modules/{mod_active}/module-issues/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/{mid}/module-issues/",
                json={"issues": ["{iss3}"]},
                auth="api_key", capture={"mi": "0/id"}, expect_status=200,
            ),
            Request(
                "DELETE", _V1 + "/modules/{mod_active}/module-issues/{iss3}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/{mid}/module-issues/{iid}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "POST", _V1 + "/modules/{mod_active}/module-issues/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/{mid}/module-issues/",
                json={"issues": []},
                auth="api_key", expect_status=400,
            ),
            # Module archive: completed 204 -> unarchive 204 ->
            # active 400.
            Request(
                "POST", _V1 + "/modules/{mod_completed}/archive/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/{mid}/archive/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _V1 + "/archived-modules/{mod_completed}/unarchive/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/archived-modules/{mid}/unarchive/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "POST", _V1 + "/modules/{mod_active}/archive/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/modules/{mid}/archive/",
                auth="api_key", expect_status=400,
            ),
        ),
    ),
)
