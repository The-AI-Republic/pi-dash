# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Direct-Postgres helpers for domains that seed data for the live backend.

``DATABASE_URL`` comes from the environment and is only required by
suites that actually seed (web_edge needs no DB). Raw-SQL seeding,
snapshots and polling live here (psycopg; no ORM, no Django imports).
``db_conn`` below imports psycopg lazily so DB-free domains never pay
for it; ``db_cursor`` offers a committing cursor for suites whose tests
never clean up: every seeded row carries a random suffix, so suites are
safe to re-run and to parallelise without cross-test interference.

Suites that seed do so straight into Postgres with plain SQL (no ORM, no
Django imports). Connections are autocommit; each test seeds
uniquely-named rows so no cleanup pass is needed for repeat runs against
the same database.
"""
from __future__ import annotations

import json
import os
from typing import Any, Sequence
import secrets
import time
import uuid
from contextlib import contextmanager
from datetime import datetime, timezone, timedelta
from typing import Iterator

import psycopg
import pytest
from psycopg.rows import dict_row
from psycopg.types.json import Json

from . import config
from . import signing

from _harness.config import database_url

KNOWN_PASSWORD = "ContractPass123!"


def get_database_url() -> str:
    try:
        return os.environ["DATABASE_URL"]
    except KeyError as exc:
        raise pytest.UsageError(
            "DATABASE_URL is not set "
            "(e.g. DATABASE_URL=postgresql://postgres@localhost/pidash)"
        ) from exc


@pytest.fixture(scope="session")
def db_conn():
    with psycopg.connect(get_database_url(), autocommit=True) as conn:
        yield conn


def connect(database_url: str | None = None) -> psycopg.Connection:
    """Open an autocommit Postgres connection (``DATABASE_URL`` by default)."""
    return psycopg.connect(database_url or get_database_url(), autocommit=True)


def fetchone(database_url: str, sql: str, params: tuple = ()) -> dict | None:
    with connect(database_url) as conn, conn.cursor(row_factory=psycopg.rows.dict_row) as cur:
        cur.execute(sql, params)
        return cur.fetchone()


def fetchall(database_url: str, sql: str, params: tuple = ()) -> list[dict]:
    with connect(database_url) as conn, conn.cursor(row_factory=psycopg.rows.dict_row) as cur:
        cur.execute(sql, params)
        return list(cur.fetchall())


def execute(database_url: str, sql: str, params: tuple = ()) -> None:
    with connect(database_url) as conn, conn.cursor() as cur:
        cur.execute(sql, params)


def new_uuid() -> str:
    return str(uuid.uuid4())


def wait_for(
    database_url: str,
    sql: str,
    params: tuple = (),
    timeout: float = 90.0,
    poll: float = 1.0,
) -> dict | None:
    """Poll until a row matches. Returns the row, or None on timeout.

    The worker executes asynchronously; every effect assertion goes
    through here instead of a fixed sleep.
    """
    deadline = time.time() + timeout
    while time.time() < deadline:
        row = fetchone(database_url, sql, params)
        if row:
            return row
        time.sleep(poll)
    return None

def _now():
    return datetime.now(timezone.utc)


class Database:
    def __init__(self, conninfo):
        self.conninfo = conninfo

    def connect(self):
        return psycopg.connect(self.conninfo)

    def reset(self):
        # Dependency order. `schedulers` rows are signal-created per
        # workspace; a missing table here fails loudly as an FK violation.
        tables = [
            "schedulers",
            "sessions",
            "workspace_members",
            "workspaces",
            "api_tokens",
            "instance_admins",
            "instance_configurations",
            "instances",
            "users",
        ]
        with self.connect() as conn:
            with conn.cursor() as cur:
                for table in tables:
                    cur.execute(f"delete from {table};")
            conn.commit()

    def make_user(self, email, *, password=KNOWN_PASSWORD, first_name="Contract", last_name="User", is_active=True, is_bot=False):
        uid = str(uuid.uuid4())
        now = _now()
        with self.connect() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    """insert into users (password, id, username, email, first_name, last_name,
                        avatar, date_joined, created_at, updated_at, last_location,
                        created_location, is_superuser, is_managed, is_password_expired,
                        is_active, is_staff, is_email_verified, is_password_autoset,
                        token, user_timezone, last_login_ip, last_logout_ip,
                        last_login_medium, last_login_uagent, is_bot, display_name,
                        is_email_valid, is_password_reset_required)
                       values (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)
                       returning password""",
                    (
                        signing.make_password_hash(password), uid, f"u-{uid[:8]}", email,
                        first_name, last_name, "", now, now, now, "", "", False, False,
                        False, is_active, False, True, False, secrets.token_hex(32),
                        "UTC", "", "", "email", "", is_bot, f"{first_name} {last_name}",
                        True, False,
                    ),
                )
                password_hash = cur.fetchone()[0]
            conn.commit()
        return {"id": uid, "email": email, "password": password, "password_hash": password_hash}

    def make_instance(self, *, name="Contract Instance", is_setup_done=True):
        iid = str(uuid.uuid4())
        now = _now()
        with self.connect() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    """insert into instances (created_at, updated_at, id, instance_name,
                        instance_id, current_version, last_checked_at,
                        is_telemetry_enabled, is_support_required, is_setup_done,
                        is_signup_screen_visited, is_verified, domain, edition,
                        is_test, is_current_version_deprecated)
                       values (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)""",
                    (
                        now, now, iid, name, f"instance-{iid[:8]}", "1.0.0", now,
                        True, True, is_setup_done, False, False, "", "PI_DASH_COMMUNITY",
                        False, False,
                    ),
                )
            conn.commit()
        return {"id": iid, "name": name}

    def make_admin(self, instance_id, user_id, *, role=20):
        aid = str(uuid.uuid4())
        now = _now()
        with self.connect() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    """insert into instance_admins
                       (created_at, updated_at, id, role, is_verified, instance_id, user_id)
                       values (%s,%s,%s,%s,%s,%s,%s)""",
                    (now, now, aid, role, True, instance_id, user_id),
                )
            conn.commit()
        return {"id": aid}

    def make_config(self, key, value, *, category="general", is_encrypted=False):
        cid = str(uuid.uuid4())
        now = _now()
        with self.connect() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    """insert into instance_configurations
                       (created_at, updated_at, id, key, value, category, is_encrypted)
                       values (%s,%s,%s,%s,%s,%s,%s)""",
                    (now, now, cid, key, value, category, is_encrypted),
                )
            conn.commit()
        return {"id": cid}

    def make_api_token(self, user_id, *, label="Contract Token", description="",
                       user_type=0, is_service=False, token=None, expired_at=None):
        tid = str(uuid.uuid4())
        now = _now()
        with self.connect() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    """insert into api_tokens
                       (created_at, updated_at, id, label, description, is_active,
                        token, user_id, user_type, expired_at, is_service,
                        allowed_rate_limit)
                       values (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)""",
                    (
                        now, now, tid, label, description, True,
                        token or secrets.token_hex(32), user_id, user_type,
                        expired_at, is_service, "60/min",
                    ),
                )
            conn.commit()
        return {"id": tid, "label": label}

    def mint_user_session(self, user, secret, *, max_age=3600):
        # App-tree auth (`session-id` cookie) reads the same `sessions` rows
        # as the admin cookie; only the cookie name differs (see http.user_client).
        return self.mint_admin_session(user, secret, max_age=max_age)

    def mint_admin_session(self, user, secret, *, max_age=3600):
        payload = signing.session_payload(user["id"], user["password_hash"], secret)
        data = signing.encode_session(payload, secret)
        key = signing.new_session_key()
        expires = _now() + timedelta(seconds=max_age)
        with self.connect() as conn:
            with conn.cursor() as cur:
                cur.execute("delete from sessions where user_id = %s;", (user["id"],))
                cur.execute(
                    """insert into sessions
                       (session_key, session_data, expire_date, user_id, device_info)
                       values (%s,%s,%s,%s,%s::jsonb)""",
                    (key, data, expires, user["id"], json.dumps(payload["device_info"])),
                )
            conn.commit()
        return key

    def make_workspace(self, name, slug, owner_id, *, timezone="UTC"):
        wid = str(uuid.uuid4())
        now = _now()
        with self.connect() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    """insert into workspaces
                       (created_at, updated_at, id, name, slug, owner_id, timezone, background_color)
                       values (%s,%s,%s,%s,%s,%s,%s,%s)""",
                    (now, now, wid, name, slug, owner_id, timezone, "#ffffff"),
                )
            conn.commit()
        return {"id": wid, "name": name, "slug": slug}

    def make_workspace_member(self, workspace_id, user_id, *, role=20):
        mid = str(uuid.uuid4())
        now = _now()
        with self.connect() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    """insert into workspace_members
                       (created_at, updated_at, id, role, member_id, workspace_id,
                        view_props, default_props, issue_props, is_active,
                        explored_features, getting_started_checklist, tips)
                       values (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)""",
                    (
                        now, now, mid, role, user_id, workspace_id,
                        json.dumps({}), json.dumps({}), json.dumps({}), True,
                        json.dumps({}), json.dumps({}), json.dumps({}),
                    ),
                )
            conn.commit()
        return {"id": mid}

class LazyDatabase:
    """Thin psycopg wrapper. Connections are lazy so ``pytest --collect-only``
    works with no database around; fixtures connect on first use.

    Named apart from ``Database`` (the/reset + make_* helper owned by the
    license suite): same ``(dsn)`` constructor shape, disjoint methods.
    """

    def __init__(self, dsn: str):
        self._dsn = dsn
        self._conn = None

    def connect(self):
        if self._conn is None or self._conn.closed:
            from psycopg.rows import dict_row

            self._conn = psycopg.connect(self._dsn, row_factory=dict_row, autocommit=True)
        return self._conn

    def execute(self, sql: str, params: Sequence[Any] | None = None):
        with self.connect().cursor() as cur:
            cur.execute(sql, params or ())
            return cur

    def fetchone(self, sql: str, params: Sequence[Any] | None = None):
        with self.connect().cursor() as cur:
            cur.execute(sql, params or ())
            return cur.fetchone()

    def fetchall(self, sql: str, params: Sequence[Any] | None = None):
        with self.connect().cursor() as cur:
            cur.execute(sql, params or ())
            return cur.fetchall()

    def fetchval(self, sql: str, params: Sequence[Any] | None = None):
        row = self.fetchone(sql, params)
        if not row:
            return None
        return next(iter(row.values()))

    def close(self):
        if self._conn is not None and not self._conn.closed:
            self._conn.close()
        self._conn = None


@contextmanager
def db_cursor() -> Iterator[psycopg.Cursor]:
    """Yield a cursor on the backend's database; commits on clean exit."""
    with psycopg.connect(database_url()) as conn:
        with conn.cursor() as cur:
            yield cur
        conn.commit()


# --- D-20 fixed-UUID seed set (PIDASHCONV-78) ---
# Shares this module with the baseline helpers above (same ``connect``
# serves both call shapes); never a per-domain fork.


ADMIN_ID = "11111111-1111-1111-1111-111111111111"
MEMBER_ID = "22222222-2222-2222-2222-222222222222"
GUEST_ID = "33333333-3333-3333-3333-333333333333"
OUTSIDER_ID = "44444444-4444-4444-4444-444444444444"

WS_A_ID = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
WS_B_ID = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"
WS_A_SLUG = "ct-ws-a"
WS_B_SLUG = "ct-ws-b"

PROJ_A_ID = "cccccccc-cccc-cccc-cccc-cccccccccccc"
PROJ_B_ID = "dddddddd-dddd-dddd-dddd-dddddddddddd"

ADMIN_TOKEN = "ct-token-admin-0001"
MEMBER_TOKEN = "ct-token-member-0002"
GUEST_TOKEN = "ct-token-guest-0003"
OUTSIDER_TOKEN = "ct-token-outsider-0004"

STATE_BACKLOG_ID = "e0000001-0000-4000-8000-000000000000"
STATE_UNSTARTED_ID = "e0000002-0000-4000-8000-000000000000"
STATE_STARTED_ID = "e0000003-0000-4000-8000-000000000000"
STATE_COMPLETED_ID = "e0000004-0000-4000-8000-000000000000"
STATE_CANCELLED_ID = "e0000005-0000-4000-8000-000000000000"

ISSUE_BACKLOG_ID = "f0000001-0000-4000-8000-000000000000"
ISSUE_UNSTARTED_ID = "f0000002-0000-4000-8000-000000000000"
ISSUE_STARTED_ID = "f0000003-0000-4000-8000-000000000000"
ISSUE_COMPLETED_ID = "f0000004-0000-4000-8000-000000000000"
ISSUE_CANCELLED_ID = "f0000005-0000-4000-8000-000000000000"

CYCLE_ACTIVE_ID = "c0000001-0000-4000-8000-000000000000"
CYCLE_COMPLETED_ID = "c0000002-0000-4000-8000-000000000000"
CYCLE_DRAFT_ID = "c0000003-0000-4000-8000-000000000000"
CYCLE_ARCHIVED_ID = "c0000004-0000-4000-8000-000000000000"

MODULE_ACTIVE_ID = "d0000001-0000-4000-8000-000000000000"
MODULE_COMPLETED_ID = "d0000002-0000-4000-8000-000000000000"
MODULE_ARCHIVED_ID = "d0000003-0000-4000-8000-000000000000"

SEED_USER_IDS = (ADMIN_ID, MEMBER_ID, GUEST_ID, OUTSIDER_ID)
SEED_WORKSPACE_IDS = (WS_A_ID, WS_B_ID)
SEED_PROJECT_IDS = (PROJ_A_ID, PROJ_B_ID)
SEED_STATE_IDS = (
    STATE_BACKLOG_ID,
    STATE_UNSTARTED_ID,
    STATE_STARTED_ID,
    STATE_COMPLETED_ID,
    STATE_CANCELLED_ID,
)
SEED_ISSUE_IDS = (
    ISSUE_BACKLOG_ID,
    ISSUE_UNSTARTED_ID,
    ISSUE_STARTED_ID,
    ISSUE_COMPLETED_ID,
    ISSUE_CANCELLED_ID,
)
SEED_CYCLE_IDS = (
    CYCLE_ACTIVE_ID,
    CYCLE_COMPLETED_ID,
    CYCLE_DRAFT_ID,
    CYCLE_ARCHIVED_ID,
)
SEED_MODULE_IDS = (MODULE_ACTIVE_ID, MODULE_COMPLETED_ID, MODULE_ARCHIVED_ID)




def _in(ids):
    return "(" + ",".join(["%s"] * len(ids)) + ")"


def reset(conn=None):
    """Delete seed rows (children first) and re-insert the seed set."""
    own = conn is None
    if own:
        conn = connect()
    try:
        with conn.cursor() as cur:
            # Children are cleared by seed-project scope (not by seed id):
            # tests create their own cycles/modules/issues through the API.
            cur.execute(
                f"DELETE FROM issue_activities WHERE project_id IN {_in(SEED_PROJECT_IDS)}",
                list(SEED_PROJECT_IDS),
            )
            cur.execute(
                f"DELETE FROM user_favorites WHERE project_id IN {_in(SEED_PROJECT_IDS)}",
                list(SEED_PROJECT_IDS),
            )
            # Eager model-activity tasks log every write against the actor.
            cur.execute(
                f"DELETE FROM api_activity_logs WHERE created_by_id IN {_in(SEED_USER_IDS)}",
                list(SEED_USER_IDS),
            )
            cur.execute(
                f"DELETE FROM module_issues WHERE project_id IN {_in(SEED_PROJECT_IDS)}",
                list(SEED_PROJECT_IDS),
            )
            cur.execute(
                f"DELETE FROM cycle_issues WHERE project_id IN {_in(SEED_PROJECT_IDS)}",
                list(SEED_PROJECT_IDS),
            )
            cur.execute(
                f"DELETE FROM issues WHERE project_id IN {_in(SEED_PROJECT_IDS)}",
                list(SEED_PROJECT_IDS),
            )
            cur.execute(
                f"DELETE FROM states WHERE project_id IN {_in(SEED_PROJECT_IDS)}",
                list(SEED_PROJECT_IDS),
            )
            cur.execute(
                f"DELETE FROM modules WHERE project_id IN {_in(SEED_PROJECT_IDS)}",
                list(SEED_PROJECT_IDS),
            )
            cur.execute(
                f"DELETE FROM cycles WHERE project_id IN {_in(SEED_PROJECT_IDS)}",
                list(SEED_PROJECT_IDS),
            )
            cur.execute(
                f"DELETE FROM api_tokens WHERE user_id IN {_in(SEED_USER_IDS)}",
                list(SEED_USER_IDS),
            )
            cur.execute(
                f"DELETE FROM project_members WHERE project_id IN {_in(SEED_PROJECT_IDS)}",
                list(SEED_PROJECT_IDS),
            )
            cur.execute(
                f"DELETE FROM projects WHERE id IN {_in(SEED_PROJECT_IDS)}",
                list(SEED_PROJECT_IDS),
            )
            cur.execute(
                f"DELETE FROM workspace_members WHERE workspace_id IN {_in(SEED_WORKSPACE_IDS)}",
                list(SEED_WORKSPACE_IDS),
            )
            cur.execute(
                f"DELETE FROM workspaces WHERE id IN {_in(SEED_WORKSPACE_IDS)}",
                list(SEED_WORKSPACE_IDS),
            )
            cur.execute(
                f"DELETE FROM users WHERE id IN {_in(SEED_USER_IDS)}",
                list(SEED_USER_IDS),
            )
        conn.commit()
        _insert(conn)
    finally:
        if own:
            conn.close()


def _insert(conn):
    with conn.cursor() as cur:
        for uid, username, email in [
            (ADMIN_ID, "ct_admin", "ct-admin@example.com"),
            (MEMBER_ID, "ct_member", "ct-member@example.com"),
            (GUEST_ID, "ct_guest", "ct-guest@example.com"),
            (OUTSIDER_ID, "ct_outsider", "ct-outsider@example.com"),
        ]:
            cur.execute(
                """
                INSERT INTO users (
                    id, password, username, email, first_name, last_name,
                    avatar, date_joined, created_at, updated_at,
                    last_location, created_location,
                    is_superuser, is_managed, is_password_expired, is_active,
                    is_staff, is_email_verified, is_password_autoset,
                    token, user_timezone, last_login_ip, last_logout_ip,
                    last_login_medium, last_login_uagent,
                    is_bot, display_name, is_email_valid,
                    is_password_reset_required
                ) VALUES (
                    %s, '!', %s, %s, 'CT', 'User',
                    '', now(), now(), now(),
                    '', '',
                    false, false, false, true,
                    false, true, false,
                    %s, 'UTC', '', '',
                    '', '',
                    false, %s, true,
                    false
                )
                """,
                [uid, username, email, "t" * 64, username],
            )
        for wid, slug, owner in [
            (WS_A_ID, WS_A_SLUG, ADMIN_ID),
            (WS_B_ID, WS_B_SLUG, OUTSIDER_ID),
        ]:
            cur.execute(
                """
                INSERT INTO workspaces (
                    id, created_at, updated_at, name, slug,
                    created_by_id, owner_id, timezone, background_color
                ) VALUES (%s, now(), now(), %s, %s, %s, %s, 'UTC', '#ffffff')
                """,
                [wid, f"CT {slug}", slug, owner, owner],
            )
        for member, ws, role in [
            (ADMIN_ID, WS_A_ID, 20),
            (MEMBER_ID, WS_A_ID, 15),
            (GUEST_ID, WS_A_ID, 5),
            (OUTSIDER_ID, WS_B_ID, 20),
        ]:
            cur.execute(
                """
                INSERT INTO workspace_members (
                    id, created_at, updated_at, role, member_id, workspace_id,
                    view_props, default_props, issue_props, is_active,
                    explored_features, getting_started_checklist, tips
                ) VALUES (
                    gen_random_uuid(), now(), now(), %s, %s, %s,
                    '{}', '{}', '{}', true, '{}', '{}', '{}'
                )
                """,
                [role, member, ws],
            )
        for pid, ws, identifier in [
            (PROJ_A_ID, WS_A_ID, "CTP"),
            (PROJ_B_ID, WS_B_ID, "CTQ"),
        ]:
            cur.execute(
                """
                INSERT INTO projects (
                    id, created_at, updated_at, name, description, network,
                    identifier, workspace_id, cycle_view, module_view,
                    issue_views_view, page_view, intake_view,
                    archive_in, close_in, logo_props,
                    is_time_tracking_enabled, is_issue_type_enabled,
                    guest_view_all_features, timezone,
                    members_can_edit_states, repo_url, base_branch,
                    agent_default_interval_seconds, agent_default_max_ticks,
                    agent_ticking_enabled, is_default,
                    agent_review_default_interval_seconds,
                    default_agent_executor,
                    agent_test_default_interval_seconds
                ) VALUES (
                    %s, now(), now(), %s, '', 0,
                    %s, %s, true, true,
                    true, true, true,
                    0, 0, '{}',
                    false, false,
                    false, 'UTC',
                    false, '', '',
                    0, 0,
                    false, false,
                    0,
                    '',
                    0
                )
                """,
                [pid, f"CT Project {identifier}", identifier, ws],
            )
        for member, proj, ws, role in [
            (ADMIN_ID, PROJ_A_ID, WS_A_ID, 20),
            (MEMBER_ID, PROJ_A_ID, WS_A_ID, 15),
            (GUEST_ID, PROJ_A_ID, WS_A_ID, 5),
            (OUTSIDER_ID, PROJ_B_ID, WS_B_ID, 20),
        ]:
            cur.execute(
                """
                INSERT INTO project_members (
                    id, created_at, updated_at, role, member_id,
                    project_id, workspace_id, view_props, default_props,
                    sort_order, preferences, is_active
                ) VALUES (
                    gen_random_uuid(), now(), now(), %s, %s,
                    %s, %s, '{}', '{}',
                    65535, '{}', true
                )
                """,
                [role, member, proj, ws],
            )
        for token, user in [
            (ADMIN_TOKEN, ADMIN_ID),
            (MEMBER_TOKEN, MEMBER_ID),
            (GUEST_TOKEN, GUEST_ID),
            (OUTSIDER_TOKEN, OUTSIDER_ID),
        ]:
            cur.execute(
                """
                INSERT INTO api_tokens (
                    id, created_at, updated_at, token, label, user_id,
                    description, is_active, is_service, allowed_rate_limit,
                    user_type
                ) VALUES (
                    gen_random_uuid(), now(), now(), %s, %s, %s,
                    '', true, false, '100000/minute',
                    0
                )
                """,
                [token, f"CT {user}", user],
            )
        for sid, name, slug, group, seq, default in [
            (STATE_BACKLOG_ID, "CT Backlog", "ct-backlog", "backlog", 1, True),
            (
                STATE_UNSTARTED_ID,
                "CT Unstarted",
                "ct-unstarted",
                "unstarted",
                2,
                False,
            ),
            (STATE_STARTED_ID, "CT Started", "ct-started", "started", 3, False),
            (
                STATE_COMPLETED_ID,
                "CT Completed",
                "ct-completed",
                "completed",
                4,
                False,
            ),
            (
                STATE_CANCELLED_ID,
                "CT Cancelled",
                "ct-cancelled",
                "cancelled",
                5,
                False,
            ),
        ]:
            cur.execute(
                """
                INSERT INTO states (
                    id, created_at, updated_at, name, description, color,
                    slug, project_id, workspace_id, sequence, "group",
                    "default", is_triage
                ) VALUES (
                    %s, now(), now(), %s, '', '#ff0000',
                    %s, %s, %s, %s, %s, %s, false
                )
                """,
                [sid, name, slug, PROJ_A_ID, WS_A_ID, seq, group, default],
            )
        for iid, seq, state in [
            (ISSUE_BACKLOG_ID, 101, STATE_BACKLOG_ID),
            (ISSUE_UNSTARTED_ID, 102, STATE_UNSTARTED_ID),
            (ISSUE_STARTED_ID, 103, STATE_STARTED_ID),
            (ISSUE_COMPLETED_ID, 104, STATE_COMPLETED_ID),
            (ISSUE_CANCELLED_ID, 105, STATE_CANCELLED_ID),
        ]:
            cur.execute(
                """
                INSERT INTO issues (
                    id, created_at, updated_at, name, description_json,
                    priority, sequence_id, project_id, state_id,
                    workspace_id, description_html, sort_order,
                    is_draft, git_work_branch, workpad, complexity_score
                ) VALUES (
                    %s, now(), now(), %s, '{}',
                    'high', %s, %s, %s,
                    %s, '', 65535,
                    false, '', '', 0
                )
                """,
                [iid, f"CT issue {seq}", seq, PROJ_A_ID, state, WS_A_ID],
            )
        cur.execute(
            """
            INSERT INTO cycles (
                id, created_at, updated_at, name, description,
                start_date, end_date, owned_by_id, project_id, workspace_id,
                view_props, sort_order, progress_snapshot, archived_at,
                logo_props, timezone, version
            ) VALUES
            (%s, now(), now(), 'CT active cycle', '',
             now() - interval '1 day', now() + interval '6 days',
             %s, %s, %s, '{}', 65535, '{}', NULL, '{}', 'UTC', 1),
            (%s, now(), now(), 'CT completed cycle', '',
             now() - interval '10 days', now() - interval '3 days',
             %s, %s, %s, '{}', 65535, '{}', NULL, '{}', 'UTC', 1),
            (%s, now(), now(), 'CT draft cycle', '',
             NULL, NULL,
             %s, %s, %s, '{}', 65535, '{}', NULL, '{}', 'UTC', 1),
            (%s, now(), now(), 'CT archived cycle', '',
             now() - interval '20 days', now() - interval '13 days',
             %s, %s, %s, '{}', 65535, '{}', now(), '{}', 'UTC', 1)
            """,
            [
                CYCLE_ACTIVE_ID, ADMIN_ID, PROJ_A_ID, WS_A_ID,
                CYCLE_COMPLETED_ID, ADMIN_ID, PROJ_A_ID, WS_A_ID,
                CYCLE_DRAFT_ID, ADMIN_ID, PROJ_A_ID, WS_A_ID,
                CYCLE_ARCHIVED_ID, ADMIN_ID, PROJ_A_ID, WS_A_ID,
            ],
        )
        for iid in (ISSUE_STARTED_ID, ISSUE_COMPLETED_ID):
            cur.execute(
                """
                INSERT INTO cycle_issues (
                    id, created_at, updated_at, cycle_id, issue_id,
                    project_id, workspace_id
                ) VALUES (
                    gen_random_uuid(), now(), now(), %s, %s, %s, %s
                )
                """,
                [CYCLE_ACTIVE_ID, iid, PROJ_A_ID, WS_A_ID],
            )
        cur.execute(
            """
            INSERT INTO modules (
                id, created_at, updated_at, name, description,
                status, project_id, workspace_id,
                view_props, sort_order, archived_at, logo_props
            ) VALUES
            (%s, now(), now(), 'CT active module', '',
             'planned', %s, %s, '{}', 65535, NULL, '{}'),
            (%s, now(), now(), 'CT completed module', '',
             'completed', %s, %s, '{}', 65535, NULL, '{}'),
            (%s, now(), now(), 'CT archived module', '',
             'completed', %s, %s, '{}', 65535, now(), '{}')
            """,
            [
                MODULE_ACTIVE_ID, PROJ_A_ID, WS_A_ID,
                MODULE_COMPLETED_ID, PROJ_A_ID, WS_A_ID,
                MODULE_ARCHIVED_ID, PROJ_A_ID, WS_A_ID,
            ],
        )
        cur.execute(
            """
            INSERT INTO module_issues (
                id, created_at, updated_at, issue_id, module_id,
                project_id, workspace_id
            ) VALUES (
                gen_random_uuid(), now(), now(), %s, %s, %s, %s
            )
            """,
            [ISSUE_UNSTARTED_ID, MODULE_ACTIVE_ID, PROJ_A_ID, WS_A_ID],
        )
    conn.commit()


def fetch_cycle(conn, cycle_id):
    with conn.cursor() as cur:
        cur.execute(
            "SELECT id, name, deleted_at FROM cycles WHERE id = %s", [cycle_id]
        )
        return cur.fetchone()


def fetch_module(conn, module_id):
    with conn.cursor() as cur:
        cur.execute(
            "SELECT id, name, deleted_at FROM modules WHERE id = %s", [module_id]
        )
        return cur.fetchone()

# --- D-19 (PIDASHCONV-77) seeding helpers ---
# Union with the baseline above: same module, no fork. ``connect`` is
# the baseline's (its no-arg call is identical); everything below is
# added verbatim from the domain suite's first-use harness.

# Workspace / project roles (pi_dash.db.models.project.ROLE).
ADMIN = 20
MEMBER = 15
GUEST = 5

# Project network (pi_dash.db.models.project.ProjectNetwork).
NETWORK_SECRET = 0
NETWORK_PUBLIC = 2


def new_tag():
    return uuid.uuid4().hex[:8]


def create_user(conn, tag, *, first_name="Ct", last_name="User", is_bot=False):
    uid = str(uuid.uuid4())
    email = f"ct-{tag}@example.com"
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO users (id, password, username, email, first_name, last_name,
                avatar, date_joined, created_at, updated_at, last_location,
                created_location, is_superuser, is_managed, is_password_expired,
                is_active, is_staff, is_email_verified, is_password_autoset, token,
                user_timezone, last_login_ip, last_logout_ip, last_login_medium,
                last_login_uagent, is_bot, display_name, is_email_valid,
                is_password_reset_required)
            VALUES (%s, '!', %s, %s, %s, %s,
                '', now(), now(), now(), '',
                '', false, false, false,
                true, false, true, false, '',
                'UTC', '', '', '',
                '', %s, %s, true,
                false)
            """,
            (uid, email, email, first_name, last_name, is_bot, f"{first_name} {last_name}"),
        )
    conn.commit()
    return {"id": uid, "email": email}


def create_workspace(conn, tag, owner_id, *, name=None, slug=None):
    wid = str(uuid.uuid4())
    slug = slug or f"ct-{tag}"
    name = name or f"CT {tag}"
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO workspaces (id, name, slug, owner_id, created_by_id,
                updated_by_id, timezone, background_color, created_at, updated_at)
            VALUES (%s, %s, %s, %s, %s, %s, 'UTC', '', now(), now())
            """,
            (wid, name, slug, owner_id, owner_id, owner_id),
        )
    conn.commit()
    return {"id": wid, "slug": slug, "name": name}


def add_workspace_member(conn, workspace_id, user_id, role=MEMBER):
    mid = str(uuid.uuid4())
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO workspace_members (id, workspace_id, member_id, role,
                is_active, view_props, default_props, issue_props,
                explored_features, getting_started_checklist, tips,
                created_at, updated_at)
            VALUES (%s, %s, %s, %s,
                true, '{}', '{}', '{}',
                '{}', '{}', '{}',
                now(), now())
            """,
            (mid, workspace_id, user_id, role),
        )
    conn.commit()
    return {"id": mid}


def create_api_token(conn, user_id, tag, *, label=None):
    tid = str(uuid.uuid4())
    token = f"ct-{tag}-{uuid.uuid4().hex[:12]}"
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO api_tokens (id, token, label, user_type, user_id,
                description, is_active, is_service, allowed_rate_limit,
                created_at, updated_at)
            VALUES (%s, %s, %s, 0, %s, '', true, false, '', now(), now())
            """,
            (tid, token, label or f"ct-{tag}", user_id),
        )
    conn.commit()
    return {"id": tid, "token": token}


def create_project(conn, workspace_id, tag, *, name=None, identifier=None, created_by_id=None):
    pid = str(uuid.uuid4())
    name = name or f"CT Project {tag}"
    identifier = identifier or f"CT{tag.upper()}"[:12]
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
            VALUES (%s, %s, '', %s, %s,
                %s, %s, true, true,
                true, true, false, 0, 0,
                '{}', true, true,
                false, 'UTC', false,
                '', '', 0,
                0, false, false,
                0, '', 0, now(), now())
            """,
            (pid, name, NETWORK_SECRET, identifier, workspace_id, created_by_id),
        )
        cur.execute(
            """
            INSERT INTO project_identifiers (name, project_id, workspace_id,
                created_by_id, created_at, updated_at)
            VALUES (%s, %s, %s, %s, now(), now())
            """,
            (identifier, pid, workspace_id, created_by_id),
        )
    conn.commit()
    return {"id": pid, "name": name, "identifier": identifier}


def add_project_member(conn, workspace_id, project_id, user_id, role=MEMBER):
    mid = str(uuid.uuid4())
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO project_members (id, workspace_id, project_id, member_id,
                role, is_active, view_props, default_props, preferences,
                sort_order, created_at, updated_at)
            VALUES (%s, %s, %s, %s, %s, true, '{}', '{}', '{}', 0, now(), now())
            """,
            (mid, workspace_id, project_id, user_id, role),
        )
    conn.commit()
    return {"id": mid}


def create_state(conn, workspace_id, project_id, tag, *, name=None, group="backlog",
                 sequence=100.0, default=False, color="#ff0000", created_by_id=None):
    sid = str(uuid.uuid4())
    name = name or f"CT State {tag}"
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO states (id, name, description, color, slug, project_id,
                workspace_id, sequence, "group", "default", is_triage,
                created_by_id, created_at, updated_at)
            VALUES (%s, %s, '', %s, %s, %s, %s, %s, %s, %s, false, %s, now(), now())
            """,
            (sid, name, color, f"ct-{tag}", project_id, workspace_id,
             sequence, group, default, created_by_id),
        )
    conn.commit()
    return {"id": sid, "name": name}


def create_invite(conn, workspace_id, tag, *, email=None, role=MEMBER,
                  created_by_id=None, accepted=False, responded_at=None):
    iid = str(uuid.uuid4())
    email = email or f"ct-invite-{tag}@example.com"
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO workspace_member_invites (id, email, accepted, token,
                role, workspace_id, created_by_id, responded_at,
                created_at, updated_at)
            VALUES (%s, %s, %s, %s, %s, %s, %s, %s, now(), now())
            """,
            (iid, email, accepted, f"ct-invite-token-{tag}", role,
             workspace_id, created_by_id, responded_at),
        )
    conn.commit()
    return {"id": iid, "email": email}


def ensure_not_default(conn, project_id):
    """Clear the default flag so the row can be deleted.

    The first project per workspace is auto-defaulted on save(), and exactly
    one default may exist (partial unique index), so a throwaway created when
    the workspace has no default becomes undeletable. Tests clear the flag
    before deleting their own throwaways.
    """
    with conn.cursor() as cur:
        cur.execute("UPDATE projects SET is_default = false WHERE id = %s", (project_id,))


def fetch_one(conn, query, params=()):
    with conn.cursor() as cur:
        cur.execute(query, params)
        row = cur.fetchone()
        if row is None:
            return None
        return dict(zip([d.name for d in cur.description], row))


# --- PIDASHCONV-83 (app project/state/estimate oracle) ---
# Union with the baseline above (see workpad for the full rationale).
def _utcnow_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def insert_row(conn, table: str, values: dict) -> dict:
    """Insert one row, filling audit columns from live table metadata.

    ``values`` carries the columns the test cares about. ``id`` (uuid PK),
    ``created_at`` / ``updated_at`` (timestamptz), and ``deleted_at`` (NULL,
    so the row is visible to Django's default soft-delete-scoped managers)
    are filled automatically when those columns exist. Any other NOT NULL
    column without a database default must be supplied explicitly — a missing
    one raises a descriptive error instead of a bare IntegrityError.
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            """
            SELECT column_name, is_nullable, column_default, data_type
            FROM information_schema.columns WHERE table_name = %s
            """,
            (table,),
        )
        meta = {row["column_name"]: row for row in cur.fetchall()}
    if not meta:
        raise RuntimeError(f"insert_row: unknown table {table!r}")

    row = dict(values)
    if "id" in meta and "id" not in row and "uuid" in meta["id"]["data_type"]:
        row["id"] = str(uuid.uuid4())
    for stamp in ("created_at", "updated_at"):
        if stamp in meta and stamp not in row:
            row[stamp] = _utcnow_iso()
    if "deleted_at" in meta and "deleted_at" not in row:
        row["deleted_at"] = None

    unknown = [c for c in row if c not in meta]
    assert not unknown, f"insert_row({table}): unknown columns {unknown}"

    missing = [
        name
        for name, col in meta.items()
        if name not in row
        and col["is_nullable"] == "NO"
        and col["column_default"] is None
    ]
    assert not missing, (
        f"insert_row({table}): missing NOT NULL columns without defaults: "
        f"{missing} — extend the test's values dict"
    )

    columns = list(row)
    params = [Json(v) if isinstance(v, (dict, list)) else v for v in (row[c] for c in columns)]
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            f'INSERT INTO "{table}" ({", ".join(chr(34) + c + chr(34) for c in columns)}) '
            f'VALUES ({", ".join(["%s"] * len(columns))}) RETURNING *',
            params,
        )
        inserted = cur.fetchone()
    conn.commit()
    return inserted


def snapshot(conn, tables: list[str], where: dict[str, str] | None = None) -> dict:
    """Dump ``{table: {pk: row}}`` for the named tables (test-scoped WHEREs)."""
    where = where or {}
    snap = {}
    with conn.cursor(row_factory=dict_row) as cur:
        for table in tables:
            clause = f"WHERE {where[table]}" if table in where else ""
            cur.execute(f'SELECT * FROM "{table}" {clause} ORDER BY 1')
            rows = cur.fetchall()
            keyed = {}
            for row in rows:
                pk = row.get("id", object())
                keyed[str(pk)] = {k: _freeze(v) for k, v in row.items()}
            snap[table] = keyed
    return snap


def _freeze(value):
    if isinstance(value, datetime):
        return value.isoformat()
    if isinstance(value, uuid.UUID):
        return str(value)
    if isinstance(value, memoryview):
        return value.tobytes().hex()
    return value


def diff(before: dict, after: dict) -> dict:
    """Return ``{table: {"added": {...}, "removed": {...}, "changed": {...}}}``."""
    report = {}
    for table in before:
        b, a = before[table], after.get(table, {})
        added = {k: v for k, v in a.items() if k not in b}
        removed = {k: v for k, v in b.items() if k not in a}
        changed = {
            k: {"before": b[k], "after": a[k]}
            for k in b
            if k in a and b[k] != a[k]
        }
        if added or removed or changed:
            report[table] = {"added": added, "removed": removed, "changed": changed}
    return report


def wait_for_condition(predicate, timeout: float | None = None, what: str = "condition"):
    """Poll ``predicate()`` until truthy; raise with ``what`` on timeout."""
    deadline = time.monotonic() + (
        timeout if timeout is not None else config.TASK_TIMEOUT_SECONDS
    )
    last = None
    while time.monotonic() < deadline:
        last = predicate()
        if last:
            return last
        time.sleep(config.POLL_INTERVAL_SECONDS)
    raise TimeoutError(
        f"timed out waiting for {what} "
        f"after {config.TASK_TIMEOUT_SECONDS}s (last={last!r})"
    )
