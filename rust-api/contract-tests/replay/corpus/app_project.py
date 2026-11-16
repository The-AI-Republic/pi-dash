# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app-tier project write flows (PIDASHCONV-823).

Built from the ``app_project`` contract suite: test_project.py
(CRUD, members, invitations, join, project-views), test_state.py
(states), test_estimate.py (estimates). The follow-up session below
covers the legs the lifecycle session skipped: join accept/decline,
deploy boards, archive/unarchive, favorites, member preferences,
project identifiers, state update/destroy/mark-default, and the
estimate-point endpoints.
"""

from psycopg.rows import dict_row

from ..session import Request, Session

# Fixed newcomer id (seeded by _seed_extras, addressed as {newcomer}).
NEWCOMER_ID = "bbbbbbbb-0000-4000-8000-000000000002"
NEWCOMER_EMAIL = "replay-newcomer@example.com"

# Fixed ids for the follow-up session (invites, occupied state + issue).
INV_OUT_ID = "bbbbbbbb-0000-4000-8000-000000000011"
INV_MEMBER_ID = "bbbbbbbb-0000-4000-8000-000000000012"
ST_OCC_ID = "bbbbbbbb-0000-4000-8000-000000000013"
OCC_ISSUE_ID = "bbbbbbbb-0000-4000-8000-000000000014"

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


def _seed_followup(conn) -> None:
    """Invites + occupied state the follow-up session addresses by id.

    The join legs mirror the suite's invite seeding
    (``_harness.seed.project_invite``) with fixed ids: one invite for
    the outsider (a replay-ws non-member, the suite's newcomer role —
    credentials only forge for world users), one for the member (the
    decline + wrong-email legs). The occupied state + blocking issue
    mirror ``test_destroy_nonempty_state_400`` (same minimal columns).
    """
    from _harness import seed as seed_helpers

    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM projects WHERE identifier = 'RPJ'")
        project_id = str(cur.fetchone()["id"])
    seed_helpers.project_invite(
        conn, project_id, ws_id, email="replay-outsider@example.com",
        id=INV_OUT_ID,
    )
    seed_helpers.project_invite(
        conn, project_id, ws_id, email="replay-member@example.com",
        id=INV_MEMBER_ID,
    )
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO states (created_at, updated_at, id, name, description,
                color, slug, project_id, workspace_id, sequence, "group",
                "default", is_triage)
            VALUES (now(), now(), %s, 'Occupied', '', '#000000',
                'occupied', %s, %s, 0.0, 'unstarted', false, false)
            """,
            (ST_OCC_ID, project_id, ws_id),
        )
        cur.execute(
            """
            INSERT INTO issues (id, name, description_json, priority, sequence_id,
                project_id, workspace_id, description_html, sort_order, is_draft,
                git_work_branch, workpad, complexity_score, state_id,
                created_at, updated_at)
            VALUES (%s, 'Blocking issue', '{}', 'none', 902, %s, %s,
                '', 0.0, false, '', '', 0, %s, now(), now())
            """,
            (OCC_ISSUE_ID, project_id, ws_id, ST_OCC_ID),
        )
    conn.commit()


SESSIONS = (
    Session(
        name="app_project_lifecycle",
        domain="app_project",
        source="app_project/test_project.py, test_state.py, test_estimate.py",
        auth_users=("admin", "member", "outsider"),
        seed=_seed_extras,
        static_context={"newcomer": NEWCOMER_ID},
        # The invite worker job echoes the minted token (args/2):
        # each side's token must exist in its own invites table
        # (workspace_invitation pattern).
        payload_db_values={
            "pi_dash.bgtasks.project_invitation_task.project_invitation": [
                ("args/2", "project_member_invites", "token"),
            ],
        },
        # Invite tokens are timestamped JWTs, minted per backend, so
        # the stored column compares blanked (workspace_member_invites
        # pattern); the queued token is still validated against each
        # side's own table above.
        generated_columns={
            "project_member_invites": ["token"],
        },
        # NOTE (rebase): PIDASHCONV-870 landed (fced7590); the default
        # pod pin tripped stale and was removed.
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
                auth="admin", expect_status=200,
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
                # The token leaf is a per-backend JWT: it compares via
                # captures (workspace GET-invites pattern). No
                # invite_link interpolation here (model fields only),
                # so no regex blanks are needed.
                auth="admin", capture={"inv": "0/id", "itok": "0/token"},
                expect_status=200,
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
    Session(
        name="app_project_followup",
        domain="app_project",
        source="app_project/test_project.py (join, deploy boards, "
        "archive, favorites, preferences, identifiers), test_state.py "
        "(update, mark-default, destroy), test_estimate.py (points)",
        auth_users=("admin", "member", "outsider"),
        seed=_seed_followup,
        static_context={
            "inv_out": INV_OUT_ID,
            "inv_member": INV_MEMBER_ID,
            "st_occ": ST_OCC_ID,
        },
        # The join view stamps responded_at=now() on accept/decline
        # (each backend's own now, ms apart).
        timestamp_columns={"project_member_invites": ["responded_at"]},
        requests=(
            # States: create -> patch -> mark-default flip out and back
            # -> destroy the spare -> default 400 -> nonempty 400.
            Request(
                "POST", _P + "/states/",
                route="/api/workspaces/{slug}/projects/{project_id}/states/",
                json={"name": "Contract X", "color": "#FF0000",
                      "group": "unstarted", "sequence": 45000},
                auth="admin", capture={"st_x": "id"}, expect_status=200,
            ),
            Request(
                "PATCH", _P + "/states/{st_x}/",
                route="/api/workspaces/{slug}/projects/{project_id}/states/{pk}/",
                json={"color": "#123456", "description": "updated"},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _P + "/states/{st_x}/mark-default/",
                route="/api/workspaces/{slug}/projects/{project_id}/states/{pk}/mark-default/",
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", _P + "/states/{state_backlog}/mark-default/",
                route="/api/workspaces/{slug}/projects/{project_id}/states/{pk}/mark-default/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _P + "/states/{st_x}/",
                route="/api/workspaces/{slug}/projects/{project_id}/states/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _P + "/states/{state_backlog}/",
                route="/api/workspaces/{slug}/projects/{project_id}/states/{pk}/",
                auth="admin", expect_status=400,
            ),
            Request(
                "DELETE", _P + "/states/{st_occ}/",
                route="/api/workspaces/{slug}/projects/{project_id}/states/{pk}/",
                auth="admin", expect_status=400,
            ),
            # Estimate points: estimate with one point, add a point,
            # missing-key 400, patch, destroy (keys rearrange), drop
            # the estimate.
            Request(
                "POST", _P + "/estimates/",
                route="/api/workspaces/{slug}/projects/{project_id}/estimates/",
                json={"estimate": {"name": "Est2", "type": "points"},
                      "estimate_points": [{"key": 1, "value": "1", "description": ""}]},
                auth="admin", capture={"est2": "id", "p1": "points/0/id"},
                expect_status=200,
            ),
            Request(
                "POST", _P + "/estimates/{est2}/estimate-points/",
                route="/api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/estimate-points/",
                json={"key": 3, "value": "3"},
                auth="admin", capture={"p3": "id"}, expect_status=200,
            ),
            Request(
                "POST", _P + "/estimates/{est2}/estimate-points/",
                route="/api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/estimate-points/",
                json={"value": "3"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _P + "/estimates/{est2}/estimate-points/{p1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/estimate-points/{pk}/",
                json={"value": "updated"},
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _P + "/estimates/{est2}/estimate-points/{p1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/estimate-points/{pk}/",
                json={},
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _P + "/estimates/{est2}/",
                route="/api/workspaces/{slug}/projects/{project_id}/estimates/{pk}/",
                auth="admin", expect_status=204,
            ),
            # Join: retrieve the invite, outsider accepts (workspace +
            # project memberships), member declines, wrong email 403.
            Request(
                "GET", _P + "/join/{inv_out}/",
                route="/api/workspaces/{slug}/projects/{project_id}/join/{pk}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _P + "/join/{inv_out}/",
                route="/api/workspaces/{slug}/projects/{project_id}/join/{pk}/",
                json={"email": "replay-outsider@example.com", "accepted": True},
                auth="outsider", expect_status=200,
            ),
            Request(
                "POST", _P + "/join/{inv_member}/",
                route="/api/workspaces/{slug}/projects/{project_id}/join/{pk}/",
                json={"email": "replay-member@example.com", "accepted": False},
                auth="member", expect_status=200,
            ),
            Request(
                "POST", _P + "/join/{inv_member}/",
                route="/api/workspaces/{slug}/projects/{project_id}/join/{pk}/",
                json={"email": "someone-else@example.com", "accepted": True},
                auth="admin", expect_status=403,
            ),
            # Deploy boards: get_or_create 200, patch, delete.
            Request(
                "POST", _P + "/project-deploy-boards/",
                route="/api/workspaces/{slug}/projects/{project_id}/project-deploy-boards/",
                json={"is_comments_enabled": True, "is_votes_enabled": True},
                auth="admin", capture={"board": "id", "anchor": "anchor"},
                expect_status=200,
            ),
            Request(
                "PATCH", _P + "/project-deploy-boards/{board}/",
                route="/api/workspaces/{slug}/projects/{project_id}/project-deploy-boards/{pk}/",
                json={"is_reactions_enabled": True},
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _P + "/project-deploy-boards/{board}/",
                route="/api/workspaces/{slug}/projects/{project_id}/project-deploy-boards/{pk}/",
                auth="admin", expect_status=204,
            ),
            # Favorites: create + destroy by project id.
            Request(
                "POST", "/api/workspaces/{ws}/user-favorite-projects/",
                route="/api/workspaces/{slug}/user-favorite-projects/",
                json={"project": "{project}"},
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", "/api/workspaces/{ws}/user-favorite-projects/{project}/",
                route="/api/workspaces/{slug}/user-favorite-projects/{pk}/",
                auth="admin", expect_status=204,
            ),
            # Member preferences patch.
            Request(
                "PATCH", _P + "/preferences/member/{admin}/",
                route="/api/workspaces/{slug}/projects/{project_id}/preferences/member/{pk}/",
                json={"theme": "dark"},
                auth="admin", expect_status=200,
            ),
            # Identifiers: unknown-name 204, live-project 400.
            Request(
                "DELETE", "/api/workspaces/{ws}/project-identifiers/",
                route="/api/workspaces/{slug}/project-identifiers/",
                json={"name": "NOPEZZZ"},
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", "/api/workspaces/{ws}/project-identifiers/",
                route="/api/workspaces/{slug}/project-identifiers/",
                json={"name": "RPJ"},
                auth="admin", expect_status=400,
            ),
            # Archive cycle last (adjacent legs; nothing project-scoped
            # runs while RPJ is archived).
            Request(
                "POST", _P + "/archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/archive/",
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _P + "/archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/archive/",
                auth="admin", expect_status=204,
            ),
        ),
    ),
)
