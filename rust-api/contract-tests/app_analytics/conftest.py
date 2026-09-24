"""Fixtures: seed the analytics world straight into Postgres, forge sessions.

World (slugs prefixed ``an-`` so teardown only touches our rows):

- workspaces ``an-ws`` (primary) and ``an-other`` (second tenant)
- ``an-ws``: admin (ADMIN), member (MEMBER), guest (GUEST); project ``AN``
  with the same three as project members; states backlog/completed;
  issues I1 (backlog/high/5pts), I2 (backlog/urgent/3pts), I3
  (completed/medium/8pts, completed now); analytic view AV1 storing
  ``query={"workspace__slug": "an-ws"}`` with
  ``query_dict={"x_axis": "priority", "y_axis": "issue_count"}``
- ``an-other``: outsider (ADMIN) + project ``AN2`` + state + 1 issue, so
  tenant-isolation tests have foreign data to leak (they must not)

Dates are dynamic (``now``) so year/month-sensitive aggregations stay
deterministic: I3 completes in the current year and month.
"""

import base64
import hashlib
import uuid
from datetime import datetime, timezone

import pytest

from _harness.auth import authed_client, forge_session_key, get_secret_key
from _harness.client import base_url  # noqa: F401  (re-exported fixture)
from _harness.db import db_conn, get_database_url  # noqa: F401

NOW = datetime.now(timezone.utc)
PW_SALT = "contract93seed"
_PW_DK = hashlib.pbkdf2_hmac("sha256", b"contract93-password", PW_SALT.encode(), 10000)
PASSWORD_FIELD = "pbkdf2_sha256$10000$%s$%s" % (
    PW_SALT,
    base64.b64encode(_PW_DK).decode(),
)

WS = "an-ws"
OTHER_WS = "an-other"


def _user_row(uid, username, email, display):
    return (
        PASSWORD_FIELD, uid, username, email, display, "User", "", NOW, NOW, NOW,
        "", "", False, False, False, True, False, True, False, uuid.uuid4().hex,
        "UTC", "", "", "email", "", False, display, True, False,
    )


USER_COLUMNS = (
    "password, id, username, email, first_name, last_name, avatar,"
    " date_joined, created_at, updated_at, last_location, created_location,"
    " is_superuser, is_managed, is_password_expired, is_active, is_staff,"
    " is_email_verified, is_password_autoset, token, user_timezone,"
    " last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,"
    " is_bot, display_name, is_email_valid, is_password_reset_required"
)


WORLD_TABLES = (
    "exporters", "analytic_views", "issues", "states", "project_members",
    "projects", "workspace_members",
)


def _wipe_world(cur):
    """Delete every row seeded by this suite (idempotent; prefix-scoped)."""
    cur.execute(
        "DELETE FROM sessions WHERE user_id IN "
        "(SELECT id::text FROM users WHERE username LIKE %s)",
        ("an\\_%",),
    )
    for table in WORLD_TABLES:
        cur.execute(
            "DELETE FROM " + table + " WHERE workspace_id IN "
            "(SELECT id FROM workspaces WHERE slug LIKE %s)",
            ("an-%",),
        )
    cur.execute("DELETE FROM workspaces WHERE slug LIKE %s", ("an-%",))
    cur.execute("DELETE FROM users WHERE username LIKE %s", ("an\\_%",))


@pytest.fixture(scope="session")
def seed(db_conn):
    secret = get_secret_key()
    with db_conn.cursor() as cur:
        # Idempotency: a prior interrupted run may have left an- rows behind.
        _wipe_world(cur)
    ids = {
        "admin": str(uuid.uuid4()),
        "member": str(uuid.uuid4()),
        "guest": str(uuid.uuid4()),
        "outsider": str(uuid.uuid4()),
        "ws": str(uuid.uuid4()),
        "other_ws": str(uuid.uuid4()),
        "project": str(uuid.uuid4()),
        "other_project": str(uuid.uuid4()),
        "state_backlog": str(uuid.uuid4()),
        "state_done": str(uuid.uuid4()),
        "other_state": str(uuid.uuid4()),
        "issue1": str(uuid.uuid4()),
        "issue2": str(uuid.uuid4()),
        "issue3": str(uuid.uuid4()),
        "other_issue": str(uuid.uuid4()),
        "view": str(uuid.uuid4()),
        "exporter": str(uuid.uuid4()),
        "ws_slug": WS,
        "other_slug": OTHER_WS,
    }
    with db_conn.cursor() as cur:
        for key, name, email in (
            ("admin", "an_admin", "an-admin@example.com"),
            ("member", "an_member", "an-member@example.com"),
            ("guest", "an_guest", "an-guest@example.com"),
            ("outsider", "an_outsider", "an-outsider@example.com"),
        ):
            cur.execute(
                "INSERT INTO users (%s) VALUES (%s)" % (USER_COLUMNS, ",".join(["%s"] * 29)),
                _user_row(ids[key], name, email, name),
            )
        for key, name, slug, owner in (
            ("ws", "Analytics WS", WS, "admin"),
            ("other_ws", "Other WS", OTHER_WS, "outsider"),
        ):
            cur.execute(
                "INSERT INTO workspaces (created_at, updated_at, id, name, slug,"
                " owner_id, timezone, background_color)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s)",
                (NOW, NOW, ids[key], name, slug, ids[owner], "UTC", "#ffffff"),
            )
        members = [
            ("ws", "admin", 20),
            ("ws", "member", 15),
            ("ws", "guest", 5),
            ("other_ws", "outsider", 20),
        ]
        for ws_key, user_key, role in members:
            cur.execute(
                "INSERT INTO workspace_members (created_at, updated_at, id, role,"
                " member_id, workspace_id, view_props, default_props, issue_props,"
                " is_active, explored_features, getting_started_checklist, tips)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (NOW, NOW, str(uuid.uuid4()), role, ids[user_key], ids[ws_key],
                 "{}", "{}", "{}", True, "{}", "{}", "{}"),
            )
        for key, ws_key, name, identifier in (
            ("project", "ws", "Analytics Project", "AN"),
            ("other_project", "other_ws", "Other Project", "AN2"),
        ):
            cur.execute(
                "INSERT INTO projects (created_at, updated_at, id, name, description,"
                " network, identifier, workspace_id, cycle_view, module_view,"
                " issue_views_view, page_view, intake_view, archive_in, close_in,"
                " logo_props, is_time_tracking_enabled, is_issue_type_enabled,"
                " guest_view_all_features, timezone, members_can_edit_states,"
                " repo_url, base_branch, agent_default_interval_seconds,"
                " agent_default_max_ticks, agent_ticking_enabled, is_default,"
                " agent_review_default_interval_seconds, default_agent_executor,"
                " agent_test_default_interval_seconds)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (NOW, NOW, ids[key], name, "", 0, identifier, ids[ws_key], True,
                 True, True, True, True, 0, 0, "{}", True, True, True, "UTC",
                 True, "", "", 3600, 100, True, False, 10800, "muse", 10800),
            )
        for user_key, role in (("admin", 20), ("member", 15), ("guest", 5)):
            cur.execute(
                "INSERT INTO project_members (created_at, updated_at, id, role,"
                " member_id, project_id, workspace_id, view_props, default_props,"
                " sort_order, preferences, is_active)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (NOW, NOW, str(uuid.uuid4()), role, ids[user_key], ids["project"],
                 ids["ws"], "{}", "{}", 0.0, "{}", True),
            )
        cur.execute(
            "INSERT INTO project_members (created_at, updated_at, id, role,"
            " member_id, project_id, workspace_id, view_props, default_props,"
            " sort_order, preferences, is_active)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, str(uuid.uuid4()), 20, ids["outsider"], ids["other_project"],
             ids["other_ws"], "{}", "{}", 0.0, "{}", True),
        )
        for key, name, group in (
            ("state_backlog", "AN Backlog", "backlog"),
            ("state_done", "AN Done", "completed"),
        ):
            cur.execute(
                "INSERT INTO states (created_at, updated_at, id, name, description,"
                " color, slug, project_id, workspace_id, sequence, \"group\","
                " \"default\", is_triage)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (NOW, NOW, ids[key], name, "", "#000000", name.lower().replace(" ", "-"),
                 ids["project"], ids["ws"], 0.0, group, False, False),
            )
        cur.execute(
            "INSERT INTO states (created_at, updated_at, id, name, description,"
            " color, slug, project_id, workspace_id, sequence, \"group\","
            " \"default\", is_triage)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, ids["other_state"], "Other Backlog", "", "#000000",
             "other-backlog", ids["other_project"], ids["other_ws"], 0.0,
             "backlog", False, False),
        )
        issues = [
            ("issue1", "I1 analytics", "high", "state_backlog", 5, None, "project", "ws", 1),
            ("issue2", "I2 analytics", "urgent", "state_backlog", 3, None, "project", "ws", 2),
            ("issue3", "I3 analytics", "medium", "state_done", 8, NOW, "project", "ws", 3),
            ("other_issue", "Foreign issue", "low", "other_state", 1, None,
             "other_project", "other_ws", 1),
        ]
        for key, name, priority, state, point, completed, proj, ws, seq in issues:
            cur.execute(
                "INSERT INTO issues (created_at, updated_at, id, name,"
                " description_json, priority, sequence_id, created_by_id, project_id,"
                " state_id, workspace_id, description_html, sort_order, point,"
                " completed_at, is_draft, git_work_branch, workpad, complexity_score)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (NOW, NOW, ids[key], name, "{}", priority, seq, ids["admin"],
                 ids[proj], ids[state], ids[ws], "", 0.0, point, completed,
                 False, "", "", 0),
            )
        cur.execute(
            "INSERT INTO analytic_views (created_at, updated_at, id, name,"
            " description, query, query_dict, created_by_id, workspace_id)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, ids["view"], "AV1", "seeded view",
             '{"workspace__slug": "%s"}' % WS,
             '{"x_axis": "priority", "y_axis": "issue_count"}',
             ids["admin"], ids["ws"]),
        )
        cur.execute(
            "INSERT INTO exporters (created_at, updated_at, id, project, provider,"
            " status, reason, key, token, initiated_by_id, workspace_id, type)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, ids["exporter"], [ids["project"]], "csv", "completed",
             "", "", uuid.uuid4().hex, ids["admin"], ids["ws"], "issue_exports"),
        )
        sessions = {}
        for user_key in ("admin", "member", "guest", "outsider"):
            sessions[user_key] = forge_session_key(
                db_conn, user_id=ids[user_key], password_field=PASSWORD_FIELD,
                secret=secret,
            )
    ids["sessions"] = sessions
    yield ids
    with db_conn.cursor() as cur:
        _wipe_world(cur)


@pytest.fixture(scope="session")
def clients(base_url, seed):
    handles = {}
    for user_key in ("admin", "member", "guest", "outsider"):
        handles[user_key] = authed_client(base_url, seed["sessions"][user_key])
    yield handles
    for handle in handles.values():
        handle.close()
