# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app-tier project write flows (PIDASHCONV-823).

Built from the ``app_project`` contract suite: test_project.py
(CRUD, members, invitations, join, project-views), test_state.py
(states), test_estimate.py (estimates).
"""

from psycopg.rows import dict_row

from .. import effects
from ..session import Request, Session

# Fixed newcomer id (seeded by _seed_extras, addressed as {newcomer}).
NEWCOMER_ID = "bbbbbbbb-0000-4000-8000-000000000002"
NEWCOMER_EMAIL = "replay-newcomer@example.com"

_P = "/api/workspaces/{ws}/projects/{project}"
_P2 = "/api/workspaces/{ws}/projects/{proj2}"


def _seed_extras(conn) -> None:
    """Newcomer user (fixed id) + join/invite preconditions.

    The users INSERT mirrors ``_harness.db.create_user`` (same values);
    only the id is fixed so requests can address it.
    """
    from _harness import db as db_helpers
    from _harness import seed as seed_helpers

    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()["id"])
        cur.execute(
            "SELECT id FROM projects WHERE identifier = 'RPJ'"
        )
        project_id = str(cur.fetchone()["id"])
        cur.execute(
            "SELECT id FROM users WHERE email = 'replay-outsider@example.com'"
        )
        outsider_id = str(cur.fetchone()["id"])
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
            VALUES (%s, '!', %s, %s, 'Replay', 'Newcomer',
                '', now(), now(), now(), '',
                '', false, false, false,
                true, false, true, false, '',
                'UTC', '', '', '',
                '', false, 'Replay Newcomer', true,
                false)
            """,
            (NEWCOMER_ID, NEWCOMER_EMAIL, NEWCOMER_EMAIL),
        )
        cur.execute(
            "UPDATE projects SET network = 2 WHERE id = %s", (project_id,)
        )
    conn.commit()
    db_helpers.add_workspace_member(conn, ws_id, NEWCOMER_ID, db_helpers.MEMBER)
    db_helpers.add_workspace_member(conn, ws_id, outsider_id, db_helpers.MEMBER)
    seed_helpers.project_invite(conn, project_id, ws_id)


SESSIONS = (
    Session(
        name="app_project_lifecycle",
        domain="app_project",
        source="app_project/test_project.py, test_state.py, test_estimate.py",
        auth_users=("admin", "member", "outsider"),
        seed=_seed_extras,
        static_context={"newcomer": NEWCOMER_ID},
        # Known divergence: project create skips the post_save default
        # pod (PIDASHCONV-870). Pins the exact normalised row; fails if
        # the row stops appearing Django-only (fix landed -> remove).
        known_extra_rows=[
            ("pod", "PIDASHCONV-870", {
                "created_at": effects.VOLATILE_PLACEHOLDER,
                "created_by_id": None,
                "deleted_at": None,
                "description": "Auto-created default pod. Add tier pods anytime.",
                "id": effects.VOLATILE_PLACEHOLDER,
                "is_default": True,
                "name": "RPJ2_pod_1",
                "project_id": "{proj2}",
                "updated_at": effects.VOLATILE_PLACEHOLDER,
                "workspace_id": "{ws_id}",
            }),
        ],
        requests=(
            Request(
                "POST", "/api/workspaces/{ws}/projects/",
                route="/api/workspaces/{slug}/projects/",
                json={"name": "Replay Proj2", "identifier": "RPJ2"},
                auth="admin", capture={"proj2": "id"}, expect_status=201,
            ),
            Request(
                "POST", "/api/workspaces/{ws}/projects/",
                route="/api/workspaces/{slug}/projects/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", "/api/workspaces/{ws}/projects/",
                route="/api/workspaces/{slug}/projects/",
                json={"name": "Dupe", "identifier": "RPJ"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _P2 + "/",
                route="/api/workspaces/{slug}/projects/{pk}/",
                json={"name": "Renamed", "description": "new desc"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PUT", _P2 + "/",
                route="/api/workspaces/{slug}/projects/{pk}/",
                json={"name": "Full", "identifier": "RPJ2"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _P2 + "/states/",
                route="/api/workspaces/{slug}/projects/{project_id}/states/",
                json={"name": "Contract New", "color": "#FF0000",
                      "group": "unstarted", "sequence": 45000},
                auth="admin", capture={"state": "id"}, expect_status=200,
            ),
            Request(
                "POST", _P2 + "/estimates/",
                route="/api/workspaces/{slug}/projects/{project_id}/estimates/",
                json={"estimate": {"name": "Est", "type": "points"},
                      "estimate_points": [{"key": 1, "value": "1", "description": "one"}]},
                auth="admin", capture={"est": "id", "estpoint": "points/0/id"},
                expect_status=200,
            ),
            Request(
                "PATCH", _P2 + "/",
                route="/api/workspaces/{slug}/projects/{pk}/",
                json={"estimate": "{est}"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _P2 + "/estimates/{est}/",
                route="/api/workspaces/{slug}/projects/{project_id}/estimates/{pk}/",
                json={"estimate": {"name": "Renamed Est"},
                      "estimate_points": [{"id": "{estpoint}", "key": 5, "value": "5"}]},
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _P2 + "/estimates/{est}/",
                route="/api/workspaces/{slug}/projects/{project_id}/estimates/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", _P + "/members/",
                route="/api/workspaces/{slug}/projects/{project_id}/members/",
                json={"members": [{"member_id": "{newcomer}", "role": 15}]},
                auth="admin", capture={"pm": "0/id"}, expect_status=201,
            ),
            Request(
                "PATCH", _P + "/members/{pm}/",
                route="/api/workspaces/{slug}/projects/{project_id}/members/{pk}/",
                json={"role": 5},
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _P + "/project-members/me/",
                route="/api/workspaces/{slug}/projects/{project_id}/project-members/me/",
                auth="admin", capture={"own": "id"}, expect_status=200,
            ),
            Request(
                "DELETE", _P + "/members/{own}/",
                route="/api/workspaces/{slug}/projects/{project_id}/members/{pk}/",
                auth="admin", expect_status=400,
            ),
            Request(
                "DELETE", _P + "/members/{pm}/",
                route="/api/workspaces/{slug}/projects/{project_id}/members/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", _P + "/members/leave/",
                route="/api/workspaces/{slug}/projects/{project_id}/members/leave/",
                auth="member", expect_status=204,
            ),
            Request(
                "POST", _P + "/invitations/",
                route="/api/workspaces/{slug}/projects/{project_id}/invitations/",
                json={"emails": [{"email": "bug@example.com", "role": 15}]},
                auth="admin", expect_status=500,
            ),
            Request(
                "POST", _P + "/invitations/",
                route="/api/workspaces/{slug}/projects/{project_id}/invitations/",
                json={"emails": []},
                auth="admin", expect_status=400,
            ),
            Request(
                "GET", _P + "/invitations/",
                route="/api/workspaces/{slug}/projects/{project_id}/invitations/",
                auth="admin", capture={"inv": "0/id"}, expect_status=200,
            ),
            Request(
                "DELETE", _P + "/invitations/{inv}/",
                route="/api/workspaces/{slug}/projects/{project_id}/invitations/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", "/api/users/me/workspaces/{ws}/projects/invitations/",
                route="/api/users/me/workspaces/{slug}/projects/invitations/",
                json={"project_ids": ["{project}"]},
                auth="outsider", expect_status=201,
            ),
            Request(
                "POST", _P + "/project-views/",
                route="/api/workspaces/{slug}/projects/{project_id}/project-views/",
                json={"view_props": {"kanban": True}, "sort_order": 3},
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", _P2 + "/archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/archive/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _P2 + "/",
                route="/api/workspaces/{slug}/projects/{pk}/",
                json={"name": "Nope"},
                auth="admin", expect_status=400,
            ),
            Request(
                "DELETE", _P2 + "/",
                route="/api/workspaces/{slug}/projects/{pk}/",
                auth="admin", expect_status=204,
            ),
        ),
    ),
)
