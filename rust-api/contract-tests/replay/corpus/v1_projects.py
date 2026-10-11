# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: api-v1 project write flows (PIDASHCONV-823).

Built from the ``v1_projects`` contract suite: test_projects.py
(CRUD, archive, conflicts), test_members.py (members +
project-members aliases), test_states.py (states), test_invites.py
(workspace invitations). API-key auth throughout (the suite's
``http`` harness); process_logs jobs compare with 920/937 pins.
"""

from psycopg.rows import dict_row

from ..session import Request, Session

# Fixed ids seeded by _seed_extras (addressed via static_context).
NEWCOMER_ID = "dddddddd-0000-4000-8000-000000000001"
NEWCOMER2_ID = "dddddddd-0000-4000-8000-000000000002"
ACCEPTED_INV_ID = "dddddddd-0000-4000-8000-000000000003"

_V1 = "/api/v1/workspaces/{ws}/projects/{project}"
_V1P = "/api/v1/workspaces/{ws}/projects"


def _seed_extras(conn) -> None:
    """Newcomers + identifier row + accepted invite (fixed ids).

    The users INSERT mirrors ``_harness.db.create_user`` (same values);
    only the ids are fixed so requests can address them. The accepted
    invite mirrors ``_harness.db.create_invite`` plus the suite's
    accepted flip. (``create_project`` already writes RPJ's
    project_identifiers row, so the identifier-taken 409 leg needs no
    seed — unlike the v1_work_items move target, whose raw INSERT
    predates that helper behaviour.)
    """
    from _harness import db as db_helpers

    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM projects WHERE identifier = 'RPJ'")
        project_id = str(cur.fetchone()["id"])
        cur.execute(
            "SELECT id FROM users WHERE email = 'replay-admin@example.com'"
        )
        admin_id = str(cur.fetchone()["id"])
    with conn.cursor() as cur:
        for user_id, email in (
            (NEWCOMER_ID, "replay-v1new@example.com"),
            (NEWCOMER2_ID, "replay-v1new2@example.com"),
        ):
            cur.execute(
                """
                INSERT INTO users (id, password, username, email, first_name, last_name,
                    avatar, date_joined, created_at, updated_at, last_location,
                    created_location, is_superuser, is_managed, is_password_expired,
                    is_active, is_staff, is_email_verified, is_password_autoset, token,
                    user_timezone, last_login_ip, last_logout_ip, last_login_medium,
                    last_login_uagent, is_bot, display_name, is_email_valid,
                    is_password_reset_required)
                VALUES (%s, '!', %s, %s, 'Replay', 'V1New',
                    '', now(), now(), now(), '',
                    '', false, false, false,
                    true, false, true, false, '',
                    'UTC', '', '', '',
                    '', false, 'Replay V1New', true,
                    false)
                """,
                (user_id, email, email),
            )
        cur.execute(
            """
            INSERT INTO workspace_member_invites (id, email, accepted, token,
                role, workspace_id, created_by_id, responded_at,
                created_at, updated_at)
            VALUES (%s, 'replay-accepted@example.com', true, 'replay-accepted-token',
                15, %s, %s, now(), now(), now())
            """,
            (ACCEPTED_INV_ID, ws_id, admin_id),
        )
    conn.commit()
    db_helpers.add_workspace_member(conn, ws_id, NEWCOMER_ID, db_helpers.MEMBER)
    db_helpers.add_workspace_member(conn, ws_id, NEWCOMER2_ID, db_helpers.MEMBER)


SESSIONS = (
    Session(
        name="v1_projects_lifecycle",
        domain="v1_projects",
        source="v1_projects/test_projects.py, test_members.py, "
        "test_states.py, test_invites.py",
        auth_users=("api_key",),
        seed=_seed_extras,
        static_context={
            "newcomer": NEWCOMER_ID,
            "newcomer2": NEWCOMER2_ID,
            "accepted_inv": ACCEPTED_INV_ID,
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
        # NOTE: PIDASHCONV-868 landed as 60c5a28b (Rust now enqueues
        # process_logs) — known-absent pin removed on rebase; the
        # order gap is pinned per session (PIDASHCONV-920) and the
        # user stamps below (PIDASHCONV-937).
        # PIDASHCONV-937: mongo_log.created_by/updated_by carry the
        # API-key owner's id on Django, null on Rust (LoggerUserId is
        # never inserted outside tests). Seed-robust: the owner is
        # the world admin on every request of this session.
        payload_value_diffs={
            "pi_dash.bgtasks.logger_task.process_logs": [
                ("kwargs/mongo_log/created_by", "PIDASHCONV-937",
                 "{admin}", None),
                ("kwargs/mongo_log/updated_by", "PIDASHCONV-937",
                 "{admin}", None),
                # --- generated PIDASHCONV-920 header-order pins (one exact
                # pair per request; lengths blanked, {api_token} rendered;
                # bodyless requests also pin Django's phantom content
                # headers — same issue, second facet) ---
        # req 0 POST /api/v1/workspaces/{slug}/projects/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 1 POST /api/v1/workspaces/{slug}/projects/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 2 POST /api/v1/workspaces/{slug}/projects/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 3 POST /api/v1/workspaces/{slug}/projects/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 4 POST /api/v1/workspaces/{slug}/projects/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 5 PATCH /api/v1/workspaces/{slug}/projects/{pk}/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 6 POST /api/v1/workspaces/{slug}/projects/{pk}/archive/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Content-Length': '<replay-volatile>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Content-Length': '<replay-volatile>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        # req 7 PATCH /api/v1/workspaces/{slug}/projects/{pk}/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 8 DELETE /api/v1/workspaces/{slug}/projects/{pk}/archive/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        # req 9 DELETE /api/v1/workspaces/{slug}/projects/{pk}/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        # req 10 DELETE /api/v1/workspaces/{slug}/projects/{pk}/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        # req 11 POST /api/v1/workspaces/{slug}/projects/{pk}/members/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 12 POST /api/v1/workspaces/{slug}/projects/{pk}/members/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 13 POST /api/v1/workspaces/{slug}/projects/{pk}/members/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 14 POST /api/v1/workspaces/{slug}/projects/{pk}/members/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 15 PATCH /api/v1/workspaces/{slug}/projects/{pk}/members/{mid}/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 16 POST /api/v1/workspaces/{slug}/projects/{pk}/project-members/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 17 PATCH /api/v1/workspaces/{slug}/projects/{pk}/project-members/{mid}/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 18 DELETE /api/v1/workspaces/{slug}/projects/{pk}/project-members/{mid}/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        # req 19 DELETE /api/v1/workspaces/{slug}/projects/{pk}/members/{mid}/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        # req 20 POST /api/v1/workspaces/{slug}/projects/{pk}/states/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 21 POST /api/v1/workspaces/{slug}/projects/{pk}/states/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 22 POST /api/v1/workspaces/{slug}/projects/{pk}/states/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 23 PATCH /api/v1/workspaces/{slug}/projects/{pk}/states/{sid}/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 24 DELETE /api/v1/workspaces/{slug}/projects/{pk}/states/{sid}/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        # req 25 DELETE /api/v1/workspaces/{slug}/projects/{pk}/states/{sid}/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        # req 26 POST /api/v1/workspaces/{slug}/invitations/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 27 POST /api/v1/workspaces/{slug}/invitations/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 28 POST /api/v1/workspaces/{slug}/invitations/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 29 POST /api/v1/workspaces/{slug}/invitations/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 30 PATCH /api/v1/workspaces/{slug}/invitations/{pk}/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 31 PATCH /api/v1/workspaces/{slug}/invitations/{pk}/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 32 DELETE /api/v1/workspaces/{slug}/invitations/{pk}/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        # req 33 DELETE /api/v1/workspaces/{slug}/invitations/{pk}/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{api_token}'}"),
            ],
        },
        # The v1 create serializer mints logo_props.icon.{name,color}
        # via random.choice (api/serializers/project.py:177-185),
        # ignoring input (logo_props is not a declared field): minted
        # per backend, so the column compares blanked.
        generated_columns={"projects": ["logo_props"]},
        # NOTE (rebase): PIDASHCONV-870/892/893/894/895/896 all landed;
        # their exact pins tripped stale and were removed; this session
        # now compares the v1 create seeding and DELETE enqueues fully.
        requests=(
            # Projects: create -> name-409 -> identifier-409 (name body
            # quirk) -> seeded-identifier 409 (identifier body) ->
            # missing-identifier 400 -> patch -> archive -> archived
            # patch 400 -> unarchive -> delete; default-delete 400 on
            # RPJ (the world default).
            Request(
                "POST", _V1P + "/",
                route="/api/v1/workspaces/{slug}/projects/",
                json={"name": "Replay V1A", "identifier": "V1A"},
                auth="api_key",
                # Random icon+color minted per backend (see
                # generated_columns): captured so the stored values
                # line up in later bodies and job payloads
                # (current_instance embeds them).
                capture={"proj_a": "id",
                         "logo_name": "logo_props/icon/name",
                         "logo_color": "logo_props/icon/color"},
                expect_status=201,
            ),
            Request(
                "POST", _V1P + "/",
                route="/api/v1/workspaces/{slug}/projects/",
                json={"name": "Replay V1A", "identifier": "V1B"},
                auth="api_key", expect_status=409,
            ),
            Request(
                "POST", _V1P + "/",
                route="/api/v1/workspaces/{slug}/projects/",
                json={"name": "Replay V1C", "identifier": "V1A"},
                auth="api_key", expect_status=409,
            ),
            Request(
                "POST", _V1P + "/",
                route="/api/v1/workspaces/{slug}/projects/",
                json={"name": "Replay V1D", "identifier": "RPJ"},
                auth="api_key", expect_status=409,
            ),
            Request(
                "POST", _V1P + "/",
                route="/api/v1/workspaces/{slug}/projects/",
                json={"name": "Replay V1E"},
                auth="api_key", expect_status=400,
            ),
            Request(
                "PATCH", _V1P + "/{proj_a}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/",
                json={"description": "patched via replay"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _V1P + "/{proj_a}/archive/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/archive/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "PATCH", _V1P + "/{proj_a}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/",
                json={"description": "must not apply"},
                auth="api_key", expect_status=400,
            ),
            Request(
                "DELETE", _V1P + "/{proj_a}/archive/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/archive/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _V1P + "/{proj_a}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _V1P + "/{project}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/",
                auth="api_key", expect_status=400,
            ),
            # Members: create -> role-400 -> non-workspace-400 ->
            # missing-member 400 -> patch role; then the
            # project-members alias: create -> patch -> delete; delete
            # the members/-created row (soft).
            Request(
                "POST", _V1 + "/members/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/members/",
                json={"member": "{newcomer}", "role": 15},
                auth="api_key", capture={"pm": "id"}, expect_status=201,
            ),
            Request(
                "POST", _V1 + "/members/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/members/",
                json={"member": "{newcomer2}", "role": 99},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", _V1 + "/members/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/members/",
                json={"member": "{outsider}", "role": 15},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", _V1 + "/members/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/members/",
                json={"role": 15},
                auth="api_key", expect_status=400,
            ),
            Request(
                "PATCH", _V1 + "/members/{pm}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/members/{mid}/",
                json={"role": 5},
                auth="api_key", expect_status=200,
            ),
            Request(
                "POST", _V1 + "/project-members/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/project-members/",
                json={"member": "{newcomer2}", "role": 15},
                auth="api_key", capture={"pm2": "id"}, expect_status=201,
            ),
            Request(
                "PATCH", _V1 + "/project-members/{pm2}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/project-members/{mid}/",
                json={"role": 5},
                auth="api_key", expect_status=200,
            ),
            Request(
                "DELETE", _V1 + "/project-members/{pm2}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/project-members/{mid}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _V1 + "/members/{pm}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/members/{mid}/",
                auth="api_key", expect_status=204,
            ),
            # States: create (200, not 201) -> name-409 -> triage-400
            # -> patch -> delete; default-delete 400 on the backlog.
            Request(
                "POST", _V1 + "/states/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/states/",
                json={"name": "Replay V1St", "color": "#00ff00",
                      "group": "backlog", "sequence": 1000.0},
                auth="api_key", capture={"st": "id"}, expect_status=200,
            ),
            Request(
                "POST", _V1 + "/states/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/states/",
                json={"name": "Replay V1St", "color": "#00ff00", "group": "backlog"},
                auth="api_key", expect_status=409,
            ),
            Request(
                "POST", _V1 + "/states/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/states/",
                json={"name": "Replay Tri", "color": "#00ff00", "group": "triage"},
                auth="api_key", expect_status=400,
            ),
            Request(
                "PATCH", _V1 + "/states/{st}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/states/{sid}/",
                json={"color": "#0000ff"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "DELETE", _V1 + "/states/{st}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/states/{sid}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _V1 + "/states/{state_backlog}/",
                route="/api/v1/workspaces/{slug}/projects/{pk}/states/{sid}/",
                auth="api_key", expect_status=400,
            ),
            # Invites: create -> duplicate-400 -> malformed-400 ->
            # role-400 -> patch role -> email-immutable 400 -> delete;
            # accepted-invite delete 400.
            Request(
                "POST", "/api/v1/workspaces/{ws}/invitations/",
                route="/api/v1/workspaces/{slug}/invitations/",
                json={"email": "replay-inv1@example.com", "role": 15},
                auth="api_key", capture={"inv": "id"}, expect_status=201,
            ),
            Request(
                "POST", "/api/v1/workspaces/{ws}/invitations/",
                route="/api/v1/workspaces/{slug}/invitations/",
                json={"email": "replay-inv1@example.com", "role": 15},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", "/api/v1/workspaces/{ws}/invitations/",
                route="/api/v1/workspaces/{slug}/invitations/",
                json={"email": "not-an-email", "role": 15},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", "/api/v1/workspaces/{ws}/invitations/",
                route="/api/v1/workspaces/{slug}/invitations/",
                json={"email": "replay-inv2@example.com", "role": 99},
                auth="api_key", expect_status=400,
            ),
            Request(
                "PATCH", "/api/v1/workspaces/{ws}/invitations/{inv}/",
                route="/api/v1/workspaces/{slug}/invitations/{pk}/",
                json={"role": 5},
                auth="api_key", expect_status=200,
            ),
            Request(
                "PATCH", "/api/v1/workspaces/{ws}/invitations/{inv}/",
                route="/api/v1/workspaces/{slug}/invitations/{pk}/",
                json={"email": "replay-changed@example.com"},
                auth="api_key", expect_status=400,
            ),
            Request(
                "DELETE", "/api/v1/workspaces/{ws}/invitations/{inv}/",
                route="/api/v1/workspaces/{slug}/invitations/{pk}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", "/api/v1/workspaces/{ws}/invitations/{accepted_inv}/",
                route="/api/v1/workspaces/{slug}/invitations/{pk}/",
                auth="api_key", expect_status=400,
            ),
        ),
    ),
)
