"""Fixtures: seed the issues world straight into Postgres, forge sessions.

World (slugs prefixed ``is-`` so teardown only touches our rows):

- workspaces ``is-ws`` (primary) and ``is-other`` (second tenant)
- ``is-ws``: admin (ADMIN), member (MEMBER), guest (GUEST); projects ``IS``
  (guest_view_all_features on) and ``IS2`` (guest_view_all_features off,
  for guest-scoping pins) with the same three as project members
- ``IS`` states: backlog (default) / completed / cancelled; issues I1
  (backlog/high), I2 (backlog/urgent, child of I1), I3 (completed/medium,
  completed now), I4 (draft), I5 (archived, completed state)
- ``IS2`` state backlog (default); issues J1 (admin-owned), J2
  (guest-owned) — the guest-scoping pair
- ``is-other``: outsider (ADMIN) + project ``ISX`` + state + issue X1, so
  tenant-isolation tests have foreign data to leak (they must not)
- relations: I1 blocked_by I3, I1 relates_to I2; link + comment C1 (with a
  comment reaction) + issue reaction + subscriber + label L1 on I1;
  activity row, file asset, version rows, PR + code-review links on I1

Dates are dynamic (``now``) so time-sensitive rendering stays
deterministic.
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
PW_SALT = "contract84seed"
_PW_DK = hashlib.pbkdf2_hmac("sha256", b"contract84-password", PW_SALT.encode(), 10000)
PASSWORD_FIELD = "pbkdf2_sha256$10000$%s$%s" % (
    PW_SALT,
    base64.b64encode(_PW_DK).decode(),
)

WS = "is-ws"
OTHER_WS = "is-other"


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

PROJECT_COLUMNS = (
    "created_at, updated_at, id, name, description,"
    " network, identifier, workspace_id, cycle_view, module_view,"
    " issue_views_view, page_view, intake_view, archive_in, close_in,"
    " logo_props, is_time_tracking_enabled, is_issue_type_enabled,"
    " guest_view_all_features, timezone, members_can_edit_states,"
    " repo_url, base_branch, agent_default_interval_seconds,"
    " agent_default_max_ticks, agent_ticking_enabled, is_default,"
    " agent_review_default_interval_seconds, default_agent_executor,"
    " agent_test_default_interval_seconds"
)

PROJECT_VALUES = (
    NOW, NOW, None, None, "", 0, None, None, True,
    True, True, True, True, 0, 0, "{}", True, True, True, "UTC",
    True, "", "", 3600, 100, True, False, 10800, "muse", 10800,
)


def _project_row(pid, name, identifier, ws_id, guest_view_all):
    row = list(PROJECT_VALUES)
    row[2] = pid
    row[3] = name
    row[6] = identifier
    row[7] = ws_id
    row[18] = guest_view_all
    return tuple(row)


ISSUE_COLUMNS = (
    "created_at, updated_at, id, name,"
    " description_json, priority, sequence_id, created_by_id, project_id,"
    " state_id, workspace_id, description_html, sort_order, point,"
    " completed_at, is_draft, git_work_branch, workpad, complexity_score"
)


def _issue_row(iid, name, priority, seq, creator, proj, state, ws,
               completed=None, draft=False):
    return (
        NOW, NOW, iid, name, "{}", priority, seq, creator,
        proj, state, ws, "", 0.0, None,
        completed, draft, "", "", 0,
    )


WORLD_TABLES = (
    "issue_sequences",
    "project_user_properties",
    "issue_reactions", "comment_reactions", "issue_subscribers",
    "issue_links", "issue_relations", "issue_labels", "labels",
    "issue_activities", "issue_comments", "descriptions",
    "issue_versions",
    "issue_description_versions", "file_assets",
    "github_pull_request_links", "git_code_review_links",
    "issue_assignees", "issues", "states", "project_members",
    "projects", "workspace_members",
)


def _wipe_world(cur):
    """Delete every row seeded by this suite (idempotent; prefix-scoped)."""
    cur.execute(
        "DELETE FROM sessions WHERE user_id IN "
        "(SELECT id::text FROM users WHERE username LIKE %s)",
        ("is\\_%",),
    )
    for table in WORLD_TABLES:
        cur.execute(
            "DELETE FROM " + table + " WHERE workspace_id IN "
            "(SELECT id FROM workspaces WHERE slug LIKE %s)",
            ("is-%",),
        )
    cur.execute("DELETE FROM workspaces WHERE slug LIKE %s", ("is-%",))
    cur.execute("DELETE FROM users WHERE username LIKE %s", ("is\\_%",))


@pytest.fixture(scope="session")
def seed(db_conn):
    secret = get_secret_key()
    with db_conn.cursor() as cur:
        # Idempotency: a prior interrupted run may have left is- rows behind.
        _wipe_world(cur)
    ids = {
        "admin": str(uuid.uuid4()),
        "member": str(uuid.uuid4()),
        "guest": str(uuid.uuid4()),
        "outsider": str(uuid.uuid4()),
        "ws": str(uuid.uuid4()),
        "other_ws": str(uuid.uuid4()),
        "project": str(uuid.uuid4()),
        "project2": str(uuid.uuid4()),
        "other_project": str(uuid.uuid4()),
        "state_backlog": str(uuid.uuid4()),
        "state_done": str(uuid.uuid4()),
        "state_cancelled": str(uuid.uuid4()),
        "state2_backlog": str(uuid.uuid4()),
        "other_state": str(uuid.uuid4()),
        "issue1": str(uuid.uuid4()),
        "issue2": str(uuid.uuid4()),
        "issue3": str(uuid.uuid4()),
        "issue4": str(uuid.uuid4()),
        "issue5": str(uuid.uuid4()),
        "j1": str(uuid.uuid4()),
        "j2": str(uuid.uuid4()),
        "other_issue": str(uuid.uuid4()),
        "label1": str(uuid.uuid4()),
        "label2": str(uuid.uuid4()),
        "comment1": str(uuid.uuid4()),
        "asset1": str(uuid.uuid4()),
        "asset2": str(uuid.uuid4()),
        "version1": str(uuid.uuid4()),
        "desc_version1": str(uuid.uuid4()),
        "pr_link": str(uuid.uuid4()),
        "review_link": str(uuid.uuid4()),
        "review_link2": str(uuid.uuid4()),
        "ws_slug": WS,
        "other_slug": OTHER_WS,
    }
    with db_conn.cursor() as cur:
        for key, name, email in (
            ("admin", "is_admin", "is-admin@example.com"),
            ("member", "is_member", "is-member@example.com"),
            ("guest", "is_guest", "is-guest@example.com"),
            ("outsider", "is_outsider", "is-outsider@example.com"),
        ):
            cur.execute(
                "INSERT INTO users (%s) VALUES (%s)" % (USER_COLUMNS, ",".join(["%s"] * 29)),
                _user_row(ids[key], name, email, name),
            )
        for key, name, slug, owner in (
            ("ws", "Issues WS", WS, "admin"),
            ("other_ws", "Other WS", OTHER_WS, "outsider"),
        ):
            cur.execute(
                "INSERT INTO workspaces (created_at, updated_at, id, name, slug,"
                " owner_id, timezone, background_color)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s)",
                (NOW, NOW, ids[key], name, slug, ids[owner], "UTC", "#ffffff"),
            )
        for ws_key, user_key, role in (
            ("ws", "admin", 20),
            ("ws", "member", 15),
            ("ws", "guest", 5),
            ("other_ws", "outsider", 20),
        ):
            cur.execute(
                "INSERT INTO workspace_members (created_at, updated_at, id, role,"
                " member_id, workspace_id, view_props, default_props, issue_props,"
                " is_active, explored_features, getting_started_checklist, tips)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (NOW, NOW, str(uuid.uuid4()), role, ids[user_key], ids[ws_key],
                 "{}", "{}", "{}", True, "{}", "{}", "{}"),
            )
        for key, ws_key, name, identifier, guest_all in (
            ("project", "ws", "Issues Project", "IS", True),
            ("project2", "ws", "Issues Project 2", "IS2", False),
            ("other_project", "other_ws", "Other Project", "ISX", True),
        ):
            cur.execute(
                "INSERT INTO projects (%s) VALUES (%s)" % (
                    PROJECT_COLUMNS, ",".join(["%s"] * 30)),
                _project_row(ids[key], name, identifier, ids[ws_key], guest_all),
            )
        for proj_key, ws_key, user_key, role in (
            ("project", "ws", "admin", 20),
            ("project", "ws", "member", 15),
            ("project", "ws", "guest", 5),
            ("project2", "ws", "admin", 20),
            ("project2", "ws", "member", 15),
            ("project2", "ws", "guest", 5),
            ("other_project", "other_ws", "outsider", 20),
        ):
            cur.execute(
                "INSERT INTO project_members (created_at, updated_at, id, role,"
                " member_id, project_id, workspace_id, view_props, default_props,"
                " sort_order, preferences, is_active)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (NOW, NOW, str(uuid.uuid4()), role, ids[user_key],
                 ids[proj_key], ids[ws_key], "{}", "{}", 0.0, "{}", True),
            )
        for key, proj_key, ws_key, name, group, default in (
            ("state_backlog", "project", "ws", "IS Backlog", "backlog", True),
            ("state_done", "project", "ws", "IS Done", "completed", False),
            ("state_cancelled", "project", "ws", "IS Cancelled", "cancelled", False),
            ("state2_backlog", "project2", "ws", "IS2 Backlog", "backlog", True),
            ("other_state", "other_project", "other_ws", "Other Backlog",
             "backlog", True),
        ):
            cur.execute(
                "INSERT INTO states (created_at, updated_at, id, name, description,"
                " color, slug, project_id, workspace_id, sequence, \"group\","
                " \"default\", is_triage)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (NOW, NOW, ids[key], name, "", "#000000",
                 name.lower().replace(" ", "-"),
                 ids[proj_key], ids[ws_key], 0.0, group, default, False),
            )
        for key, name, priority, seq, creator, proj, state, ws, completed, draft in (
            ("issue1", "I1 parent", "high", 1, "admin", "project",
             "state_backlog", "ws", None, False),
            ("issue2", "I2 child", "urgent", 2, "admin", "project",
             "state_backlog", "ws", None, False),
            ("issue3", "I3 done", "medium", 3, "admin", "project",
             "state_done", "ws", NOW, False),
            ("issue4", "I4 draft", "low", 4, "admin", "project",
             "state_backlog", "ws", None, True),
            ("issue5", "I5 archived", "high", 5, "admin", "project",
             "state_done", "ws", NOW, False),
            ("j1", "J1 admin owned", "medium", 1, "admin", "project2",
             "state2_backlog", "ws", None, False),
            ("j2", "J2 guest owned", "low", 2, "guest", "project2",
             "state2_backlog", "ws", None, False),
            ("other_issue", "Foreign issue", "low", 1, "outsider",
             "other_project", "other_state", "other_ws", None, False),
        ):
            cur.execute(
                "INSERT INTO issues (%s) VALUES (%s)" % (
                    ISSUE_COLUMNS, ",".join(["%s"] * 19)),
                _issue_row(ids[key], name, priority, seq, ids[creator],
                           ids[proj], ids[state], ids[ws], completed, draft),
            )
        # I2 is the sub-issue of I1; I5 is archived.
        cur.execute(
            "UPDATE issues SET parent_id = %s WHERE id = %s",
            (ids["issue1"], ids["issue2"]),
        )
        cur.execute(
            "UPDATE issues SET archived_at = %s WHERE id = %s",
            (NOW.date(), ids["issue5"]),
        )
        # Labels: L1 on I1.
        for key, name, color in (("label1", "L1", "#ff0000"),
                                 ("label2", "L2", "#0000ff")):
            cur.execute(
                "INSERT INTO labels (created_at, updated_at, id, name,"
                " description, created_by_id, project_id, updated_by_id,"
                " workspace_id, color, sort_order)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (NOW, NOW, ids[key], name, "", ids["admin"], ids["project"],
                 ids["admin"], ids["ws"], color, 0.0),
            )
        cur.execute(
            "INSERT INTO issue_labels (created_at, updated_at, id, created_by_id,"
            " issue_id, label_id, project_id, updated_by_id, workspace_id)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, str(uuid.uuid4()), ids["admin"], ids["issue1"],
             ids["label1"], ids["project"], ids["admin"], ids["ws"]),
        )
        # Member assigned to I1.
        cur.execute(
            "INSERT INTO issue_assignees (created_at, updated_at, id, assignee_id,"
            " created_by_id, issue_id, project_id, updated_by_id, workspace_id)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, str(uuid.uuid4()), ids["member"], ids["admin"],
             ids["issue1"], ids["project"], ids["admin"], ids["ws"]),
        )
        # Comment C1 on I1 (by admin) + member reaction on it.
        cur.execute(
            "INSERT INTO issue_comments (created_at, updated_at, id,"
            " comment_html, comment_stripped, comment_json, created_by_id,"
            " issue_id, project_id, updated_by_id, workspace_id, actor_id,"
            " access, attachments, speaker_type, speaker_label, labels)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, ids["comment1"], "<p>seed comment</p>", "seed comment",
             "{}", ids["admin"], ids["issue1"], ids["project"], ids["admin"],
             ids["ws"], ids["admin"], "INTERNAL", "{}", "human", "", "{}"),
        )
        cur.execute(
            "INSERT INTO comment_reactions (created_at, updated_at, id, reaction,"
            " actor_id, comment_id, created_by_id, project_id, updated_by_id,"
            " workspace_id)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, str(uuid.uuid4()), "+1", ids["member"],
             ids["comment1"], ids["member"], ids["project"], ids["member"],
             ids["ws"]),
        )
        # Issue reaction + subscriber on I1.
        cur.execute(
            "INSERT INTO issue_reactions (created_at, updated_at, id, reaction,"
            " actor_id, created_by_id, issue_id, project_id, updated_by_id,"
            " workspace_id)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, str(uuid.uuid4()), "rocket", ids["member"],
             ids["member"], ids["issue1"], ids["project"], ids["member"],
             ids["ws"]),
        )
        cur.execute(
            "INSERT INTO issue_subscribers (created_at, updated_at, id,"
            " created_by_id, issue_id, project_id, subscriber_id, updated_by_id,"
            " workspace_id)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, str(uuid.uuid4()), ids["member"], ids["issue1"],
             ids["project"], ids["member"], ids["member"], ids["ws"]),
        )
        # Relations: I1 blocked_by I3, I1 relates_to I2.
        for relation_type, issue_key, related_key in (
            ("blocked_by", "issue1", "issue3"),
            ("relates_to", "issue1", "issue2"),
        ):
            cur.execute(
                "INSERT INTO issue_relations (created_at, updated_at, id,"
                " relation_type, created_by_id, issue_id, project_id,"
                " related_issue_id, updated_by_id, workspace_id)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (NOW, NOW, str(uuid.uuid4()), relation_type, ids["admin"],
                 ids[issue_key], ids["project"], ids[related_key],
                 ids["admin"], ids["ws"]),
            )
        # Link on I1.
        cur.execute(
            "INSERT INTO issue_links (created_at, updated_at, id, title, url,"
            " created_by_id, issue_id, project_id, updated_by_id, workspace_id,"
            " metadata)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, str(uuid.uuid4()), "Seed spec",
             "https://example.com/seed-spec", ids["admin"], ids["issue1"],
             ids["project"], ids["admin"], ids["ws"], "{}"),
        )
        # One activity row on I1.
        cur.execute(
            "INSERT INTO issue_activities (created_at, updated_at, id, verb,"
            " field, old_value, new_value, created_by_id, issue_id, project_id,"
            " updated_by_id, workspace_id, actor_id, epoch, comment,"
            " attachments)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, str(uuid.uuid4()), "updated", "priority", "low",
             "high", ids["admin"], ids["issue1"], ids["project"],
             ids["admin"], ids["ws"], ids["admin"], int(NOW.timestamp()),
             "", "{}"),
        )
        # V1-style uploaded asset + V2-style pending asset on I1.
        cur.execute(
            "INSERT INTO file_assets (created_at, updated_at, id, attributes,"
            " asset, created_by_id, updated_by_id, workspace_id, is_deleted,"
            " is_archived, entity_type, is_uploaded, issue_id, project_id,"
            " size, user_id)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, ids["asset1"], '{"name": "seed.txt"}', "seed.txt",
             ids["admin"], ids["admin"], ids["ws"], False, False,
             "ISSUE_ATTACHMENT", True, ids["issue1"], ids["project"], 12,
             ids["admin"]),
        )
        cur.execute(
            "INSERT INTO file_assets (created_at, updated_at, id, attributes,"
            " asset, created_by_id, updated_by_id, workspace_id, is_deleted,"
            " is_archived, entity_type, is_uploaded, issue_id, project_id,"
            " size, user_id)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, ids["asset2"], '{"name": "pending.bin"}',
             "pending.bin", ids["admin"], ids["admin"], ids["ws"], False,
             False, "ISSUE_ATTACHMENT", False, ids["issue1"], ids["project"],
             10, ids["admin"]),
        )
        # Version rows on I1.
        cur.execute(
            "INSERT INTO issue_versions (created_at, updated_at, id, name,"
            " priority, sequence_id, last_saved_at, created_by_id, issue_id,"
            " project_id, updated_by_id, workspace_id, assignees, labels,"
            " modules, meta, properties, is_draft, sort_order, owned_by_id)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,"
            "%s)",
            (NOW, NOW, ids["version1"], "I1 parent", "high", 1, NOW,
             ids["admin"], ids["issue1"], ids["project"], ids["admin"],
             ids["ws"], "{}", "{}", "{}", "{}", "{}", False, 0.0,
             ids["admin"]),
        )
        cur.execute(
            "INSERT INTO issue_description_versions (created_at, updated_at, id,"
            " description_html, description_stripped, description_json,"
            " last_saved_at, created_by_id, issue_id, project_id, updated_by_id,"
            " workspace_id, owned_by_id)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, ids["desc_version1"], "<p>seed</p>", "seed", "{}",
             NOW, ids["admin"], ids["issue1"], ids["project"], ids["admin"],
             ids["ws"], ids["admin"]),
        )
        # PR + code-review links on I1.
        cur.execute(
            "INSERT INTO github_pull_request_links (created_at, updated_at, id,"
            " repo_owner, repo_name, pr_number, url, title, state, merged, draft,"
            " created_by_id, issue_id, project_id, updated_by_id, workspace_id)"
            " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
            (NOW, NOW, ids["pr_link"], "octo", "demo", 7,
             "https://github.com/octo/demo/pull/7", "Seed PR", "open",
             False, False, ids["admin"], ids["issue1"], ids["project"],
             ids["admin"], ids["ws"]),
        )
        for key, iid, url, title in (
            ("review_link", "7", "https://github.com/octo/demo/pull/7",
             "Seed review"),
            ("review_link2", "8", "https://github.com/octo/demo/pull/8",
             "Seed review 2"),
        ):
            cur.execute(
                "INSERT INTO git_code_review_links (created_at, updated_at, id,"
                " provider, host_url, namespace, repo_name, external_iid, url,"
                " title, state, merged, draft, created_by_id, issue_id,"
                " project_id, updated_by_id, workspace_id, external_id,"
                " repo_external_id, metadata)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,"
                "%s,%s,%s,%s)",
                (NOW, NOW, ids[key], "github",
                 "https://github.com", "octo", "demo", iid, url, title,
                 "open", False, False, ids["admin"], ids["issue1"],
                 ids["project"], ids["admin"], ids["ws"], "", "", "{}"),
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
