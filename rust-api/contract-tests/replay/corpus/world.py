# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Base world seed for write-replay sessions (PIDASHCONV-823).

Seeds the shared world every corpus session starts from, on top of a
freshly migrated database: users, workspaces + members, projects +
members, states, API tokens, the license instance + admin, and one
active webhook row per workspace (pointing at the placeholder sink
host, rewritten per backend at restore time).

Built from the existing contract fixtures: the user/workspace/project/
state/token rows reuse ``_harness.db`` factories (the same calls the
``app_*``/``v1_*`` suites seed with), the password hash reuses
``_harness.seed.make_password``, the instance/admin rows reuse
``_harness.db.Database`` (as ``license/conftest.py`` does), and the
webhook row reuses ``_harness.seed.webhook``.

Random ids and ``now()`` are fine here: the seeded template database is
dumped to ``snapshot.sql`` once, and every session restores that exact
snapshot into both copies (plus a Django→Rust clone after session
extras), so both backends always start bit-identical.
"""

from __future__ import annotations

import psycopg

from _harness import db as db_helpers
from _harness import seed as seed_helpers
from _harness.db import ADMIN, GUEST, MEMBER

from .. import effects

#: Password every seeded user signs in with (auth-domain sessions).
PASSWORD = "replay-password-1"

#: Fixed slugs/identifiers sessions address (looked up, never assumed).
WS_SLUG = "replay-ws"
OTHER_WS_SLUG = "replay-other"
PROJECT_IDENTIFIER = "RPJ"
OTHER_PROJECT_IDENTIFIER = "RPX"

#: Users seeded, keyed by the credential name sessions use.
USER_KEYS = ("admin", "member", "guest", "outsider")


def seed_world(conninfo: str) -> dict:
    """Seed the base world; return the world's credential/address book."""
    pw_field = seed_helpers.make_password(PASSWORD)
    database = db_helpers.Database(conninfo)
    world: dict = {"users": {}, "workspaces": {}, "projects": {}, "states": {}}
    with psycopg.connect(conninfo, autocommit=True) as conn:
        users = {}
        specs = (
            ("admin", "Replay", "Admin", "replay-admin@example.com"),
            ("member", "Replay", "Member", "replay-member@example.com"),
            ("guest", "Replay", "Guest", "replay-guest@example.com"),
            ("outsider", "Replay", "Outsider", "replay-outsider@example.com"),
        )
        for key, first, last, email in specs:
            row = db_helpers.create_user(
                conn, f"replay-{key}", first_name=first, last_name=last, email=email
            )
            with conn.cursor() as cur:
                cur.execute(
                    "UPDATE users SET password = %s, token = '' WHERE id = %s",
                    (pw_field, row["id"]),
                )
            users[key] = {"id": row["id"], "email": email, "password_field": pw_field}
        world["users"] = users

        ws = db_helpers.create_workspace(
            conn, "replay-ws", users["admin"]["id"], name="Replay WS", slug=WS_SLUG
        )
        other_ws = db_helpers.create_workspace(
            conn, "replay-other", users["outsider"]["id"],
            name="Replay Other", slug=OTHER_WS_SLUG,
        )
        world["workspaces"] = {"ws": ws, "other": other_ws}
        db_helpers.add_workspace_member(conn, ws["id"], users["admin"]["id"], ADMIN)
        db_helpers.add_workspace_member(conn, ws["id"], users["member"]["id"], MEMBER)
        db_helpers.add_workspace_member(conn, ws["id"], users["guest"]["id"], GUEST)
        db_helpers.add_workspace_member(conn, other_ws["id"], users["outsider"]["id"], ADMIN)

        project = db_helpers.create_project(
            conn, ws["id"], "replay", name="Replay Project",
            identifier=PROJECT_IDENTIFIER, created_by_id=users["admin"]["id"],
        )
        other_project = db_helpers.create_project(
            conn, other_ws["id"], "replayx", name="Replay Other Project",
            identifier=OTHER_PROJECT_IDENTIFIER, created_by_id=users["outsider"]["id"],
        )
        # RPJ is the workspace default: project create marks the new
        # project default when none exists, and default projects cannot
        # be deleted — sessions create and delete projects freely.
        with conn.cursor() as cur:
            cur.execute(
                "UPDATE projects SET is_default = true WHERE id = %s",
                (project["id"],),
            )
        world["projects"] = {"main": project, "other": other_project}
        for key, role in (("admin", ADMIN), ("member", MEMBER), ("guest", GUEST)):
            db_helpers.add_project_member(conn, ws["id"], project["id"], users[key]["id"], role)
        db_helpers.add_project_member(
            conn, other_ws["id"], other_project["id"], users["outsider"]["id"], ADMIN
        )

        states = {}
        for name, group, default in (
            ("Backlog", "backlog", True),
            ("Completed", "completed", False),
            ("Cancelled", "cancelled", False),
        ):
            row = db_helpers.create_state(
                conn, ws["id"], project["id"], f"replay-{group}", name=name,
                group=group, default=default, created_by_id=users["admin"]["id"],
            )
            states[group] = row
        other_state = db_helpers.create_state(
            conn, other_ws["id"], other_project["id"], "replay-other",
            name="Backlog", group="backlog", default=True,
            created_by_id=users["outsider"]["id"],
        )
        states["other_backlog"] = other_state
        world["states"] = states

        token = db_helpers.create_api_token(conn, users["admin"]["id"], "replay")
        world["api_token"] = token["token"]

        hook_base = f"http://{effects.WEBHOOK_PLACEHOLDER_HOST}"
        world["webhooks"] = [
            seed_helpers.webhook(
                conn, ws["id"], f"{hook_base}/hook/replay-ws",
                issue=True, issue_comment=True,
            ),
            seed_helpers.webhook(
                conn, other_ws["id"], f"{hook_base}/hook/replay-other",
                issue=True,
            ),
        ]

    instance = database.make_instance(name="Replay Instance")
    database.make_admin(instance["id"], users["admin"]["id"])
    world["instance"] = instance
    return world


def template_context(world: dict) -> dict:
    """World values sessions address as {placeholders} (identical both sides)."""
    users = world["users"]
    return {
        "ws": world["workspaces"]["ws"]["slug"],
        "other_ws": world["workspaces"]["other"]["slug"],
        "ws_id": str(world["workspaces"]["ws"]["id"]),
        "other_ws_id": str(world["workspaces"]["other"]["id"]),
        "project": str(world["projects"]["main"]["id"]),
        "other_project": str(world["projects"]["other"]["id"]),
        "state_backlog": str(world["states"]["backlog"]["id"]),
        "state_completed": str(world["states"]["completed"]["id"]),
        "state_cancelled": str(world["states"]["cancelled"]["id"]),
        "admin": str(users["admin"]["id"]),
        "member": str(users["member"]["id"]),
        "guest": str(users["guest"]["id"]),
        "outsider": str(users["outsider"]["id"]),
    }


def load_world(conninfo: str) -> dict:
    """Re-read the credential/address book from a seeded database.

    The runner calls this after restoring: ids come from the snapshot,
    so this looks everything up by the fixed slugs/emails instead of
    trusting in-memory values from seeding time.
    """
    from psycopg.rows import dict_row

    world: dict = {"users": {}, "workspaces": {}, "projects": {}, "states": {}}
    with psycopg.connect(conninfo) as conn, conn.cursor(row_factory=dict_row) as cur:
        for key, email in (
            ("admin", "replay-admin@example.com"),
            ("member", "replay-member@example.com"),
            ("guest", "replay-guest@example.com"),
            ("outsider", "replay-outsider@example.com"),
        ):
            cur.execute(
                "SELECT id, password FROM users WHERE email = %s", (email,)
            )
            row = cur.fetchone()
            world["users"][key] = {
                "id": str(row["id"]),
                "email": email,
                "password_field": row["password"],
            }
        for key, slug in (("ws", WS_SLUG), ("other", OTHER_WS_SLUG)):
            cur.execute("SELECT id, slug FROM workspaces WHERE slug = %s", (slug,))
            world["workspaces"][key] = dict(cur.fetchone())
        for key, identifier in (("main", PROJECT_IDENTIFIER), ("other", OTHER_PROJECT_IDENTIFIER)):
            cur.execute(
                "SELECT id, identifier FROM projects WHERE identifier = %s", (identifier,)
            )
            world["projects"][key] = dict(cur.fetchone())
        cur.execute(
            "SELECT id, name, \"group\" FROM states WHERE \"group\" = ANY(%s)"
            " AND workspace_id = %s",
            (["backlog", "completed", "cancelled"], str(world["workspaces"]["ws"]["id"])),
        )
        for row in cur.fetchall():
            world["states"][row["group"]] = dict(row)
        cur.execute(
            "SELECT token FROM api_tokens WHERE user_id = %s ORDER BY created_at DESC LIMIT 1",
            (world["users"]["admin"]["id"],),
        )
        world["api_token"] = cur.fetchone()["token"]
    return world
