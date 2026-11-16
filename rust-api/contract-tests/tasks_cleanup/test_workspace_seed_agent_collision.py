# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-09 oracle: workspace_seed with a colliding agent username (PIDASHCONV-1701).

A non-bot ``users`` row holding the reserved ``pi_dash_agent`` username makes
every preflight bounce raise ``AgentUserCollisionError``. Django swallows it:
non-ticking seed issues never reach preflight (``service.py:226`` returns
``not-a-trigger-state`` first), and the ticking ones raise inside the handler
where ``signals.py:98`` catches and logs. The seed completes: all 7 issues,
the view, and the pages exist; ticking issues keep their seeded state with no
bounce comment (the bounce rolls back inside its own atomic block).

Rust pre-fix ran the preflight probe for every seeded issue and propagated the
error, aborting the seed after one issue. This suite fails against that build
and passes after the fix. The Rust replay is
``test_rust_replay.py::test_rust_workspace_seed_agent_collision``.

Ticker rows are deliberately unasserted: Django's reconcile write persists
outside the bounce atomic block while Rust rolls the whole drive back.
"""

import json
import uuid

from psycopg.rows import dict_row

from _harness import celery_wire, taskspec
from _harness import seed as seed_helpers
from _harness.db import insert_row
from _harness.db import wait_for_condition as wait_for

M = "pi_dash.bgtasks"
TASK = f"{M}.workspace_seed_task.workspace_seed"
AGENT_USERNAME = "pi_dash_agent"

# agent_phases.PHASES pins the literal (name, group) pairs (no Django import).
TICKING = {("In Progress", "started"), ("In Review", "review"), ("In Test", "test")}


def _seed_rows(name):
    root = taskspec.source_root() / "pi_dash" / "seeds" / "data"
    with open(root / name) as fh:
        return json.load(fh)


def _count(conn, table, where="", params=()):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(f'SELECT count(*) AS n FROM "{table}" {where}', params)
        return cur.fetchone()["n"]


def _one(conn, table, where, params=()):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(f'SELECT * FROM "{table}" {where}', params)
        return cur.fetchone()


def _project_rows(conn, table, project_id):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            f'SELECT * FROM "{table}" WHERE project_id = %s::uuid', (str(project_id),)
        )
        return cur.fetchall()


def _colliding_user_values():
    # seed_helpers.user always tag-suffixes its username, so the exact
    # reserved name is built with insert_row (same columns as seed.user).
    return {
        "username": AGENT_USERNAME,
        "email": f"{AGENT_USERNAME}@example.com",
        "password": seed_helpers.password_hash("contract-suite-password"),
        "display_name": AGENT_USERNAME,
        "first_name": "",
        "last_name": "",
        "avatar": "",
        "date_joined": seed_helpers._now_iso(),
        "is_active": True,
        "last_location": "",
        "created_location": "",
        "is_superuser": False,
        "is_managed": False,
        "is_password_expired": False,
        "is_staff": False,
        "is_email_verified": False,
        "is_password_autoset": False,
        "token": "",
        "user_timezone": "UTC",
        "last_login_ip": "127.0.0.1",
        "last_logout_ip": "127.0.0.1",
        "last_login_medium": "email",
        "last_login_uagent": "",
        "is_bot": False,
        "is_email_valid": True,
        "is_password_reset_required": False,
    }


def _hold_collision(conn):
    """Make ``pi_dash_agent`` a colliding non-bot row; return a restore thunk.

    FK-safe (the fire_tick_seam live_b8 precedent): an existing row — usually
    the real bot, already referenced by earlier bounce comments — is flipped
    non-bot and flipped back; only a row created here is deleted.
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT id, is_bot FROM users WHERE username = %s", (AGENT_USERNAME,)
        )
        existing = cur.fetchone()
    if existing is not None:

        def _restore_flip():
            with conn.cursor() as cur:
                cur.execute(
                    "UPDATE users SET is_bot = %s WHERE id = %s::uuid",
                    (existing["is_bot"], str(existing["id"])),
                )

        with conn.cursor() as cur:
            cur.execute(
                "UPDATE users SET is_bot = false WHERE id = %s::uuid",
                (str(existing["id"]),),
            )
        return _restore_flip

    insert_row(conn, "users", _colliding_user_values())

    def _restore_delete():
        with conn.cursor() as cur:
            cur.execute(
                "DELETE FROM issue_comments WHERE actor_id IN"
                " (SELECT id FROM users WHERE username = %s)",
                (AGENT_USERNAME,),
            )
            cur.execute("DELETE FROM users WHERE username = %s", (AGENT_USERNAME,))

    return _restore_delete


def test_workspace_seed_agent_collision(db_conn, broker_url):
    tag = uuid.uuid4().hex[:8]
    owner = seed_helpers.user(db_conn, f"collide1701-owner-{tag}")
    other = seed_helpers.user(db_conn, f"collide1701-other-{tag}")
    workspace = seed_helpers.workspace(
        db_conn,
        f"collide1701{tag}",
        owner["id"],
        name=f"Collision 1701 {tag}",
    )
    seed_helpers.workspace_member(db_conn, workspace["id"], owner["id"], role=5)
    seed_helpers.workspace_member(db_conn, workspace["id"], other["id"], role=10)

    seed_issues = _seed_rows("issues.json")
    seed_states = _seed_rows("states.json")
    seed_views = _seed_rows("views.json")
    seed_pages = _seed_rows("pages.json")

    restore = _hold_collision(db_conn)
    try:
        celery_wire.publish(TASK, args=[str(workspace["id"])])

        project = wait_for(
            lambda: _one(
                db_conn,
                "projects",
                "WHERE workspace_id = %s::uuid AND deleted_at IS NULL",
                (str(workspace["id"]),),
            ),
            what="collision seed project created",
        )
        wait_for(
            lambda: (
                _count(
                    db_conn,
                    "issues",
                    "WHERE project_id = %s::uuid",
                    (str(project["id"]),),
                )
                == len(seed_issues)
            )
            or None,
            what="collision seed issues created",
        )
        # Tail entities land after issues (PIDASHCONV-900 shape): poll them
        # too instead of asserting once.
        wait_for(
            lambda: (
                _count(
                    db_conn,
                    "issue_views",
                    "WHERE project_id = %s::uuid",
                    (str(project["id"]),),
                )
                == len(seed_views)
            )
            or None,
            what="collision seed views created",
        )
        wait_for(
            lambda: (
                _count(
                    db_conn,
                    "pages",
                    "WHERE workspace_id = %s::uuid AND deleted_at IS NULL",
                    (str(workspace["id"]),),
                )
                == len(seed_pages)
            )
            or None,
            what="collision seed pages created",
        )

        issues = _project_rows(db_conn, "issues", project["id"])
        assert len(issues) == len(seed_issues)
        assert {i["sequence_id"] for i in issues} == set(
            range(1, len(seed_issues) + 1)
        )
        assert (
            _count(
                db_conn, "issue_views", "WHERE project_id = %s::uuid", (str(project["id"]),)
            )
            == len(seed_views)
        )
        assert (
            _count(
                db_conn,
                "pages",
                "WHERE workspace_id = %s::uuid AND deleted_at IS NULL",
                (str(workspace["id"]),),
            )
            == len(seed_pages)
        )

        # Ticking seeds keep their seeded state: Django raises inside the
        # handler and swallows, so no bounce lands.
        ticking_seed_states = {
            s["id"] for s in seed_states if (s["name"], s.get("group")) in TICKING
        }
        by_sequence = {i["sequence_id"]: i for i in issues}
        ticking_n = [
            n
            for n, row in enumerate(seed_issues, start=1)
            if row["state_id"] in ticking_seed_states
        ]
        assert ticking_n, "seed data must carry at least one ticking-state issue"
        for n in ticking_n:
            issue = by_sequence[n]
            with db_conn.cursor(row_factory=dict_row) as cur:
                cur.execute(
                    'SELECT "group" FROM "states" WHERE id = %s::uuid',
                    (str(issue["state_id"]),),
                )
                group = cur.fetchone()["group"]
            assert group in ("started", "review", "test"), (
                f"issue {issue['id']} must keep its ticking state"
            )
        with db_conn.cursor(row_factory=dict_row) as cur:
            cur.execute(
                "SELECT count(*) AS n FROM issue_comments c"
                " JOIN issues i ON i.id = c.issue_id"
                " WHERE i.project_id = %s::uuid"
                " AND c.comment_html LIKE '%%Agent run skipped%%'",
                (str(project["id"]),),
            )
            assert cur.fetchone()["n"] == 0, "no bounce comment may land"
    finally:
        restore()
