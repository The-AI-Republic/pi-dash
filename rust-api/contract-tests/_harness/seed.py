"""Organisation seeding: users, workspaces, memberships, projects.

Column lists mirror the Django schema (``users``, ``workspaces``,
``workspace_members``, ``projects``, ``project_members``). Rows are inserted
with raw SQL, which bypasses model ``post_save`` signals by design: no
builtin schedulers are seeded and no default pod is created, so each test
sees exactly the rows it inserted.

Roles: ADMIN=20, MEMBER=15, GUEST=5.
"""

from __future__ import annotations

import datetime
import uuid

ADMIN = 20
MEMBER = 15
GUEST = 5


def now_iso() -> str:
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def new_id() -> str:
    return str(uuid.uuid4())


def create_user(conn, *, email: str, username: str, password_field: str) -> dict:
    uid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO users (id, password, username, first_name, last_name,
            avatar, date_joined, created_at, updated_at, last_location,
            created_location, is_superuser, is_managed, is_password_expired,
            is_active, is_staff, is_email_verified, is_password_autoset, token,
            user_timezone, last_login_ip, last_logout_ip, last_login_medium,
            last_login_uagent, is_bot, display_name, is_email_valid,
            is_password_reset_required, email)
        VALUES (%s,%s,%s,'First','Last','',%s,%s,%s,'','',false,false,false,
            true,false,false,false,'tok','UTC','','','','ua',false,
            'Display','t',false,%s)""",
        (uid, password_field, username, now, now, now, email),
    )
    return {"id": uid, "email": email, "password_field": password_field}


def create_workspace(conn, *, slug: str, name: str, owner_id: str) -> dict:
    wid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO workspaces (id, name, slug, owner_id, timezone,
            background_color, created_at, updated_at)
        VALUES (%s,%s,%s,%s,'UTC','#ffffff',%s,%s)""",
        (wid, name, slug, owner_id, now, now),
    )
    return {"id": wid, "slug": slug}


def add_workspace_member(
    conn, *, workspace_id: str, user_id: str, role: int, active: bool = True
) -> str:
    mid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO workspace_members (id, role, member_id, workspace_id,
            view_props, default_props, issue_props, is_active,
            explored_features, getting_started_checklist, tips,
            created_at, updated_at)
        VALUES (%s,%s,%s,%s,'{}','{}','{}',%s,'{}','{}','{}',%s,%s)""",
        (mid, role, user_id, workspace_id, active, now, now),
    )
    return mid


def create_project(
    conn, *, workspace_id: str, identifier: str, name: str, created_by_id: str
) -> dict:
    pid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO projects (id, name, description, network, identifier,
            workspace_id, cycle_view, module_view, issue_views_view, page_view,
            intake_view, archive_in, close_in, logo_props,
            is_time_tracking_enabled, is_issue_type_enabled,
            guest_view_all_features, timezone, members_can_edit_states,
            repo_url, base_branch, agent_default_interval_seconds,
            agent_default_max_ticks, agent_ticking_enabled, is_default,
            agent_review_default_interval_seconds, default_agent_executor,
            agent_test_default_interval_seconds, created_at, updated_at,
            created_by_id)
        VALUES (%s,%s,'Seeded for contract tests',1,%s,%s,
            true,true,true,true,true,30,30,'{}',
            true,true,false,'UTC',true,'','main',60,10,true,false,
            60,'local_runner',60,%s,%s,%s)""",
        (pid, name, identifier, workspace_id, now, now, created_by_id),
    )
    return {"id": pid, "identifier": identifier}


def add_project_member(
    conn,
    *,
    project_id: str,
    workspace_id: str,
    user_id: str,
    role: int,
    active: bool = True,
) -> str:
    mid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO project_members (id, role, project_id, workspace_id,
            member_id, view_props, default_props, sort_order, preferences,
            is_active, created_at, updated_at)
        VALUES (%s,%s,%s,%s,%s,'{}','{}',65535,'{}',%s,%s,%s)""",
        (mid, role, project_id, workspace_id, user_id, active, now, now),
    )
    return mid
