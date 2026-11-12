# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app workspace write flows (PIDASHCONV-823).

Built from the ``app_workspace`` contract suite:
test_workspace_core.py (CRUD, members, invitations, join,
join-requests) and test_workspace_extras.py (themes, drafts,
favorites, quick-links, stickies, home/sidebar prefs, user-props,
export). Session-cookie auth (admin/member/guest/outsider), like the
suite — no API-key legs, so no 868 pin.
"""


from psycopg.rows import dict_row

from ..session import Request, Session


# Fixed anchor issue for the extras session (seq 901, clear of
# API-created sequences — draft-to-issue mints from the same counter
# both sides).
ANCHOR_ID = "77777777-0000-4000-8000-000000000011"

# Fixed ids seeded by _seed_lifecycle (addressed via static_context).
N4_MID = "77777777-0000-4000-8000-000000000001"
INV_PATCH_ID = "77777777-0000-4000-8000-000000000002"
INV_DEL_ID = "77777777-0000-4000-8000-000000000003"
INV_JOIN_ID = "77777777-0000-4000-8000-000000000004"
JR1_ID = "77777777-0000-4000-8000-000000000007"
JR2_ID = "77777777-0000-4000-8000-000000000008"
JR3_ID = "77777777-0000-4000-8000-000000000009"

_WS = "/api/workspaces/{ws}"
_ME_JR = "/api/users/me/workspaces/join-requests/"
_ME_INV = "/api/users/me/workspaces/invitations/"

def _seed_users_and_profiles(cur, emails):
    """Raw-SQL users + profiles (mirrors make_user/make_profile)."""
    ids = {}
    for email in emails:
        cur.execute(
            """
            INSERT INTO users (id, password, username, email, first_name, last_name,
                avatar, date_joined, created_at, updated_at, last_location,
                created_location, is_superuser, is_managed, is_password_expired,
                is_active, is_staff, is_email_verified, is_password_autoset, token,
                user_timezone, last_login_ip, last_logout_ip, last_login_medium,
                last_login_uagent, is_bot, display_name, is_email_valid,
                is_password_reset_required)
            VALUES (gen_random_uuid(), '!', %s, %s, 'Replay', 'Newcomer',
                '', now(), now(), now(), '',
                '', false, false, false,
                true, false, true, false, '',
                'UTC', '', '', '',
                '', false, 'Replay Newcomer', true,
                false)
            RETURNING id
            """,
            (email, email),
        )
        user_id = str(cur.fetchone()[0])
        cur.execute(
            """
            INSERT INTO profiles (created_at, updated_at, id, theme,
                is_tour_completed, onboarding_step, is_onboarded,
                billing_address_country, has_billing_address, company_name,
                user_id, is_mobile_onboarded, mobile_onboarding_step,
                mobile_timezone_auto_set, language, is_smooth_cursor_enabled,
                start_of_the_week, is_app_rail_docked, background_color, goals,
                has_marketing_email_consent, is_navigation_tour_completed,
                is_subscribed_to_changelog, notification_view_mode,
                product_tour, settings)
            VALUES (now(), now(), gen_random_uuid(), '{}', false,
                '{"profile_complete": false}', false, 'INDIA', false, '',
                %s, false, '{}', false, 'en', false, 0, true, '#ffffff', '{}',
                false, false, false, 'full', '{}', '{}')
            """,
            (user_id,),
        )
        ids[email] = user_id
    return ids


def _seed_lifecycle(conn) -> None:
    """Newcomers + membership + invites + join-requests (fixed row ids).

    N1..N3 back the approve/deny join-request legs (approve is
    admin-called, so requesters need no credentials); N4 is the
    member-PATCH/DELETE target on replay-ws. Invites mirror
    ``make_invite`` (same columns); join-requests mirror
    ``make_join_request``.
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()["id"])
        cur.execute(
            "SELECT id, email FROM users WHERE email LIKE 'replay-%%@example.com'"
        )
        users = {r["email"]: str(r["id"]) for r in cur.fetchall()}
    admin_id = users["replay-admin@example.com"]
    outsider_id = users["replay-outsider@example.com"]
    with conn.cursor() as cur:
        newbies = _seed_users_and_profiles(cur, (
            "replay-wsn1@example.com", "replay-wsn2@example.com",
            "replay-wsn3@example.com", "replay-wsn4@example.com",
        ))
        n1, n2, n3, n4 = (newbies[e] for e in sorted(newbies))
        cur.execute(
            """
            INSERT INTO workspace_members (created_at, updated_at, id, role,
                member_id, workspace_id, view_props, default_props,
                issue_props, is_active, explored_features,
                getting_started_checklist, tips)
            VALUES (now(), now(), %s, 15, %s, %s, '{}', '{}', '{}', true,
                '{}', '{}', '{}')
            """,
            (N4_MID, n4, ws_id),
        )
        # One invite per (email, workspace): the unique constraint
        # allows a single outsider invite on replay-ws (wrong-token +
        # reject + double-respond legs); the accept leg runs on wsA
        # with an API-created invite (id + token captured from the
        # list read, whose rows drop once responded).
        for inv_id, email, token in (
            (INV_PATCH_ID, "replay-wspatch@example.com", "tok-ws-patch"),
            (INV_DEL_ID, "replay-wsdel@example.com", "tok-ws-del"),
            (INV_JOIN_ID, "replay-outsider@example.com", "tok-ws-join"),
        ):
            cur.execute(
                """
                INSERT INTO workspace_member_invites (created_at, updated_at,
                    id, email, accepted, token, role, workspace_id,
                    created_by_id, responded_at)
                VALUES (now(), now(), %s, %s, false, %s, 15, %s, %s, NULL)
                """,
                (inv_id, email, token, ws_id, admin_id),
            )
        for jr_id, requester in ((JR1_ID, n1), (JR2_ID, n2), (JR3_ID, n3)):
            cur.execute(
                """
                INSERT INTO workspace_join_requests (created_at, updated_at,
                    id, admin_email, message, role, status, requester_id,
                    workspace_id, created_by_id)
                VALUES (now(), now(), %s, 'replay-admin@example.com', 'let me in',
                    15, 'PENDING', %s, %s, %s)
                """,
                (jr_id, requester, ws_id, requester),
            )
    conn.commit()
    # Silence unused (outsider only addresses seeded invites by id).
    _ = outsider_id


def _seed_extras(conn) -> None:
    """Anchor issue on RPJ (fixed id, seq 901) for the favorite leg.

    World rows (replay-ws, RPJ, backlog state) already exist; the
    favorite leg addresses the anchor via static_context.
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM projects WHERE identifier = 'RPJ'")
        project_id = str(cur.fetchone()["id"])
        cur.execute(
            'SELECT id FROM states WHERE project_id = %s AND "default"',
            (project_id,),
        )
        state_id = str(cur.fetchone()["id"])
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO issues (id, name, description_json, priority, sequence_id,
                project_id, workspace_id, description_html, sort_order, is_draft,
                git_work_branch, workpad, complexity_score, state_id,
                created_at, updated_at)
            VALUES (%s, 'Anchor issue', '{}', 'none', 901, %s, %s,
                '', 0.0, false, '', '', 0, %s, now(), now())
            """,
            (ANCHOR_ID, project_id, ws_id, state_id),
        )
    conn.commit()


def _setup_lifecycle(world: dict, config) -> dict:
    """Read replay-ws membership ids (self legs + final promotion)."""
    import psycopg

    with psycopg.connect(config.django_db) as conn, conn.cursor() as cur:
        cur.execute(
            """
            SELECT m.member_id, m.id FROM workspace_members m
            JOIN workspaces w ON w.id = m.workspace_id
            WHERE w.slug = 'replay-ws'
            """
        )
        rows = {str(r[0]): str(r[1]) for r in cur.fetchall()}
    conn.close()
    users = world["users"]
    return {
        "own_mid": rows[users["admin"]["id"]],
        "member_mid": rows[users["member"]["id"]],
    }


SESSIONS = (
    Session(
        name="app_workspace_lifecycle",
        domain="app_workspace",
        source="app_workspace/test_workspace_core.py (CRUD, members, "
        "invitations, join, join-requests)",
        auth_users=("admin", "member", "guest", "outsider"),
        seed=_seed_lifecycle,
        static_context={
            "n4_mid": N4_MID,
            "inv_patch": INV_PATCH_ID,
            "inv_del": INV_DEL_ID,
            "inv_join": INV_JOIN_ID,
            "jr1": JR1_ID,
            "jr2": JR2_ID,
            "jr3": JR3_ID,
        },
        setup=_setup_lifecycle,
        setup_keys=("own_mid", "member_mid"),
        payload_time_paths={
            "pi_dash.bgtasks.issue_activities_task.issue_activity": ["kwargs/epoch"],
            # track_event stamps event_properties.created_at = now()
            # per backend (workspace_created + siblings below), and
            # invited_at / joined_at / deleted_at on the
            # invite/join/delete events.
            "pi_dash.bgtasks.event_tracking_task.track_event": [
                "kwargs/event_properties/created_at",
                "kwargs/event_properties/invited_at",
                "kwargs/event_properties/joined_at",
                "kwargs/event_properties/deleted_at",
            ],
        },
        # The invite worker job echoes the minted token (args/2):
        # each side's token must exist in its own invites table.
        payload_db_values={
            "pi_dash.bgtasks.workspace_invitation_task.workspace_invitation": [
                ("args/2", "workspace_member_invites", "token"),
            ],
        },
        # Workspace create mints background_color randomly per
        # backend; captured so later bodies substitute, stored
        # column blanked (v1_projects logo_props pattern). Invite
        # tokens are likewise minted per backend (64-hex) and never
        # exposed by the create response, so the stored column
        # compares blanked (captured tokens still substitute in
        # bodies and later requests).
        generated_columns={
            "workspaces": ["background_color"],
            "workspace_member_invites": ["token"],
        },
        # Join/request responses stamp responded_at = now() per backend.
        timestamp_columns={
            "workspace_member_invites": ["responded_at"],
            "workspace_join_requests": ["responded_at"],
        },
        # NOTE (rebase): PIDASHCONV-886 landed; the builtin
        # scheduler pins tripped stale and were removed.
        requests=(
            # Join-request creates first (only replay-ws + replay-other
            # exist, so admin-email routes to replay-ws alone):
            # create -> bad-email 400 -> own-email 400 -> ghost
            # neutral 201 -> already-member 200.
            Request(
                "POST", _ME_JR,
                route="/api/users/me/workspaces/join-requests/",
                json={"admin_email": "replay-admin@example.com",
                      "message": "let me in"},
                auth="outsider", expect_status=201,
            ),
            Request(
                "POST", _ME_JR,
                route="/api/users/me/workspaces/join-requests/",
                json={"admin_email": "not-an-email"},
                auth="outsider", expect_status=400,
            ),
            Request(
                "POST", _ME_JR,
                route="/api/users/me/workspaces/join-requests/",
                json={"admin_email": "replay-admin@example.com"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _ME_JR,
                route="/api/users/me/workspaces/join-requests/",
                json={"admin_email": "ghost@example.com"},
                auth="outsider", expect_status=201,
            ),
            Request(
                "POST", _ME_JR,
                route="/api/users/me/workspaces/join-requests/",
                json={"admin_email": "replay-admin@example.com"},
                auth="member", expect_status=200,
            ),
            # Workspace CRUD: create -> missing 400 -> duplicate
            # 400 -> bad-slug 400 -> url-name 400 -> retrieve ->
            # missing 404 -> outsider 404 -> patch -> outsider 403
            # -> member-on-replay-ws 403 -> put.
            Request(
                "POST", "/api/workspaces/",
                route="/api/workspaces/",
                json={"name": "Acme", "slug": "rp-wsa"},
                auth="admin",
                capture={"wsA": "id", "wsA_bg": "background_color"},
                expect_status=201,
            ),
            Request(
                "POST", "/api/workspaces/",
                route="/api/workspaces/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", "/api/workspaces/",
                route="/api/workspaces/",
                json={"name": "Acme 2", "slug": "replay-ws"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", "/api/workspaces/",
                route="/api/workspaces/",
                json={"name": "X", "slug": "bad slug!"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", "/api/workspaces/",
                route="/api/workspaces/",
                json={"name": "see https://evil.example/x", "slug": "acme2"},
                auth="admin", expect_status=400,
            ),
            Request(
                "GET", "/api/workspaces/rp-wsa/",
                route="/api/workspaces/{slug}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", "/api/workspaces/nope/",
                route="/api/workspaces/{slug}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "GET", "/api/workspaces/rp-wsa/",
                route="/api/workspaces/{slug}/",
                auth="outsider", expect_status=404,
            ),
            Request(
                "PATCH", "/api/workspaces/rp-wsa/",
                route="/api/workspaces/{slug}/",
                json={"name": "Acme Renamed"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", "/api/workspaces/rp-wsa/",
                route="/api/workspaces/{slug}/",
                json={"name": "Hacked"},
                auth="outsider", expect_status=403,
            ),
            Request(
                "PATCH", _WS + "/",
                route="/api/workspaces/{slug}/",
                json={"name": "Hacked"},
                auth="member", expect_status=403,
            ),
            Request(
                "PUT", "/api/workspaces/rp-wsa/",
                route="/api/workspaces/{slug}/",
                json={"name": "Acme Full", "slug": "rp-wsa"},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", "/api/workspaces/rp-wsa/workspace-views/",
                route="/api/workspaces/{slug}/workspace-views/",
                json={"view_props": {"layout": "kanban"}},
                auth="admin", expect_status=204,
            ),
            # Members on replay-ws: patch role -> patch-self 400 ->
            # non-admin 403 -> delete -> delete-self 400; leave on
            # wsA (sole admin) 400.
            Request(
                "PATCH", _WS + "/members/{n4_mid}/",
                route="/api/workspaces/{slug}/members/{pk}/",
                json={"role": 5},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _WS + "/members/{own_mid}/",
                route="/api/workspaces/{slug}/members/{pk}/",
                json={"role": 5},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _WS + "/members/{n4_mid}/",
                route="/api/workspaces/{slug}/members/{pk}/",
                json={"role": 5},
                auth="member", expect_status=403,
            ),
            Request(
                "DELETE", _WS + "/members/{n4_mid}/",
                route="/api/workspaces/{slug}/members/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _WS + "/members/{own_mid}/",
                route="/api/workspaces/{slug}/members/{pk}/",
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", "/api/workspaces/rp-wsa/members/leave/",
                route="/api/workspaces/{slug}/members/leave/",
                auth="admin", expect_status=400,
            ),
            # Invitations on replay-ws: create -> empty 400 ->
            # invalid-email 400 -> existing-member 400 -> higher-role
            # 400 (member) -> by-member 200 -> patch -> missing 404
            # -> delete.
            Request(
                "POST", _WS + "/invitations/",
                route="/api/workspaces/{slug}/invitations/",
                json={"emails": [{"email": "newbie@example.com", "role": 15}]},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _WS + "/invitations/",
                route="/api/workspaces/{slug}/invitations/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _WS + "/invitations/",
                route="/api/workspaces/{slug}/invitations/",
                json={"emails": [{"email": "nope", "role": 5}]},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _WS + "/invitations/",
                route="/api/workspaces/{slug}/invitations/",
                json={"emails": [{"email": "replay-member@example.com",
                                  "role": 5}]},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _WS + "/invitations/",
                route="/api/workspaces/{slug}/invitations/",
                json={"emails": [{"email": "boss@example.com", "role": 20}]},
                auth="member", expect_status=400,
            ),
            Request(
                "POST", _WS + "/invitations/",
                route="/api/workspaces/{slug}/invitations/",
                json={"emails": [{"email": "guest2@example.com", "role": 5}]},
                auth="member", expect_status=200,
            ),
            Request(
                "PATCH", _WS + "/invitations/{inv_patch}/",
                route="/api/workspaces/{slug}/invitations/{pk}/",
                json={"role": 5},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _WS + "/invitations/123e4567-e89b-12d3-a456-426614174000/",
                route="/api/workspaces/{slug}/invitations/{pk}/",
                json={"role": 5},
                auth="admin", expect_status=404,
            ),
            Request(
                "DELETE", _WS + "/invitations/{inv_del}/",
                route="/api/workspaces/{slug}/invitations/{pk}/",
                auth="admin", expect_status=204,
            ),
            # Joins. Replay-ws (seeded outsider invite): wrong-token
            # 403 -> reject 200 -> double-respond 400. The accept leg
            # runs on wsA (one invite per email+workspace): invite
            # guest, capture the id from the list read, guest
            # me-accepts 204 (the row drops from reads); then invite
            # outsider, capture id + token, outsider join-accepts.
            Request(
                "POST", _WS + "/invitations/{inv_join}/join/",
                route="/api/workspaces/{slug}/invitations/{pk}/join/",
                json={"token": "tok-wrong", "accepted": True},
                auth="outsider", expect_status=403,
            ),
            Request(
                "POST", _WS + "/invitations/{inv_join}/join/",
                route="/api/workspaces/{slug}/invitations/{pk}/join/",
                json={"token": "tok-ws-join", "accepted": False},
                auth="outsider", expect_status=200,
            ),
            Request(
                "POST", _WS + "/invitations/{inv_join}/join/",
                route="/api/workspaces/{slug}/invitations/{pk}/join/",
                json={"token": "tok-ws-join", "accepted": True},
                auth="outsider", expect_status=400,
            ),
            Request(
                "POST", "/api/workspaces/rp-wsa/invitations/",
                route="/api/workspaces/{slug}/invitations/",
                json={"emails": [{"email": "replay-guest@example.com",
                                  "role": 5}]},
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", "/api/workspaces/rp-wsa/invitations/",
                route="/api/workspaces/{slug}/invitations/",
                auth="admin", capture={"ginv": "0/id", "gtok": "0/token"},
                # invite_link interpolates the row id plus the
                # timestamped-JWT token (necessarily per-backend):
                # blank those substrings; the id/token leaves
                # themselves compare via captures, and the link
                # shape (path, params, slug) still compares.
                body_regex_blanks=[
                    "invitation_id=[0-9a-f-]{36}",
                    "token=[A-Za-z0-9_-]+\\.[A-Za-z0-9_-]+\\.[A-Za-z0-9_-]+",
                ],
                expect_status=200,
            ),
            Request(
                "POST", _ME_INV,
                route="/api/users/me/workspaces/invitations/",
                json={"invitations": ["{ginv}"]},
                auth="guest", expect_status=204,
            ),
            Request(
                "POST", "/api/workspaces/rp-wsa/invitations/",
                route="/api/workspaces/{slug}/invitations/",
                json={"emails": [{"email": "replay-outsider@example.com",
                                  "role": 15}]},
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", "/api/workspaces/rp-wsa/invitations/",
                route="/api/workspaces/{slug}/invitations/",
                auth="admin",
                capture={"oinv": "0/id", "otok": "0/token"},
                body_regex_blanks=[
                    "invitation_id=[0-9a-f-]{36}",
                    "token=[A-Za-z0-9_-]+\\.[A-Za-z0-9_-]+\\.[A-Za-z0-9_-]+",
                ],
                expect_status=200,
            ),
            Request(
                "POST", "/api/workspaces/rp-wsa/invitations/{oinv}/join/",
                route="/api/workspaces/{slug}/invitations/{pk}/join/",
                json={"token": "{otok}", "accepted": True},
                auth="outsider", expect_status=200,
            ),
            # Join-request review (newcomer requesters, admin-called):
            # approve -> approve-twice 400 -> deny -> deny-non-admin
            # 403.
            Request(
                "POST", _WS + "/join-requests/{jr1}/approve/",
                route="/api/workspaces/{slug}/join-requests/{pk}/approve/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _WS + "/join-requests/{jr1}/approve/",
                route="/api/workspaces/{slug}/join-requests/{pk}/approve/",
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _WS + "/join-requests/{jr2}/deny/",
                route="/api/workspaces/{slug}/join-requests/{pk}/deny/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _WS + "/join-requests/{jr3}/deny/",
                route="/api/workspaces/{slug}/join-requests/{pk}/deny/",
                auth="member", expect_status=403,
            ),
            # Leave + destroy: promote member to 2nd admin, admin
            # leaves replay-ws, admin (still wsA member) destroys wsA.
            Request(
                "PATCH", _WS + "/members/{member_mid}/",
                route="/api/workspaces/{slug}/members/{pk}/",
                json={"role": 20},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _WS + "/members/leave/",
                route="/api/workspaces/{slug}/members/leave/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", "/api/workspaces/rp-wsa/",
                route="/api/workspaces/{slug}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", "/api/workspaces/rp-wsa/",
                route="/api/workspaces/{slug}/",
                auth="admin", expect_status=404,
            ),
        ),
    ),
    Session(
        name="app_workspace_extras",
        domain="app_workspace",
        source="app_workspace/test_workspace_extras.py (themes, drafts, "
        "favorites, quick-links, stickies, home/sidebar prefs, "
        "user-props, export) + test_workspace_core.py (workspace-views)",
        auth_users=("admin",),
        seed=_seed_extras,
        static_context={"anchor": ANCHOR_ID},
        payload_time_paths={
            "pi_dash.bgtasks.issue_activities_task.issue_activity": ["kwargs/epoch"],
        },
        # Sticky create mints background_color randomly per backend
        # (workspace-create pattern); captured so bodies substitute,
        # stored column blanked.
        generated_columns={
            "stickies": ["background_color"],
        },
        requests=(
            # Themes: create -> list -> patch -> delete -> 404.
            Request(
                "POST", _WS + "/workspace-themes/",
                route="/api/workspaces/{slug}/workspace-themes/",
                json={"name": "T1", "deleted_at": None},
                auth="admin",
                capture={"tid": "id"},
                expect_status=201,
            ),
            Request(
                "GET", _WS + "/workspace-themes/",
                route="/api/workspaces/{slug}/workspace-themes/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _WS + "/workspace-themes/{tid}/",
                route="/api/workspaces/{slug}/workspace-themes/{pk}/",
                json={"name": "T2"},
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _WS + "/workspace-themes/{tid}/",
                route="/api/workspaces/{slug}/workspace-themes/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", _WS + "/workspace-themes/{tid}/",
                route="/api/workspaces/{slug}/workspace-themes/{pk}/",
                auth="admin", expect_status=404,
            ),
            # Drafts: create -> list -> create-with-project ->
            # patch-204 -> convert-no-project 400 -> delete x2.
            # (No convert-success leg: Rust 500s where Django 201s —
            # PIDASHCONV-904 keeps the repro, 875 precedent.)
            Request(
                "POST", _WS + "/draft-issues/",
                route="/api/workspaces/{slug}/draft-issues/",
                json={"name": "D1"},
                auth="admin",
                capture={"d1": "id"},
                expect_status=201,
            ),
            Request(
                "GET", _WS + "/draft-issues/",
                route="/api/workspaces/{slug}/draft-issues/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _WS + "/draft-issues/",
                route="/api/workspaces/{slug}/draft-issues/",
                json={"name": "D2", "project_id": "{project}"},
                auth="admin",
                capture={"d2": "id"},
                expect_status=201,
            ),
            Request(
                "PATCH", _WS + "/draft-issues/{d1}/",
                route="/api/workspaces/{slug}/draft-issues/{pk}/",
                json={"name": "D1b"},
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", _WS + "/draft-to-issue/{d1}/",
                route="/api/workspaces/{slug}/draft-to-issue/{pk}/",
                json={"name": "I1"},
                auth="admin", expect_status=400,
            ),
            Request(
                "DELETE", _WS + "/draft-issues/{d1}/",
                route="/api/workspaces/{slug}/draft-issues/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _WS + "/draft-issues/{d2}/",
                route="/api/workspaces/{slug}/draft-issues/{pk}/",
                auth="admin", expect_status=204,
            ),
            # Favorites: folder -> missing-type 400 -> bad-identifier
            # 400 -> issue-fav -> duplicate-returns-existing -> list
            # -> patch -> group-read -> delete x2.
            Request(
                "POST", _WS + "/user-favorites/",
                route="/api/workspaces/{slug}/user-favorites/",
                json={"entity_type": "folder", "name": "F1",
                      "is_folder": True},
                auth="admin",
                capture={"fid": "id"},
                expect_status=200,
            ),
            Request(
                "POST", _WS + "/user-favorites/",
                route="/api/workspaces/{slug}/user-favorites/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _WS + "/user-favorites/",
                route="/api/workspaces/{slug}/user-favorites/",
                json={"entity_type": "project",
                      "entity_identifier": "x-1", "name": "F1"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _WS + "/user-favorites/",
                route="/api/workspaces/{slug}/user-favorites/",
                json={"entity_type": "issue",
                      "entity_identifier": "{anchor}", "name": "FI"},
                auth="admin",
                capture={"fiid": "id"},
                expect_status=200,
            ),
            Request(
                "POST", _WS + "/user-favorites/",
                route="/api/workspaces/{slug}/user-favorites/",
                json={"entity_type": "issue",
                      "entity_identifier": "{anchor}", "name": "FI"},
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _WS + "/user-favorites/",
                route="/api/workspaces/{slug}/user-favorites/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _WS + "/user-favorites/{fid}/",
                route="/api/workspaces/{slug}/user-favorites/{pk}/",
                json={"name": "F2"},
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _WS + "/user-favorites/{fid}/group/",
                route="/api/workspaces/{slug}/user-favorites/{pk}/group/",
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _WS + "/user-favorites/{fid}/",
                route="/api/workspaces/{slug}/user-favorites/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _WS + "/user-favorites/{fiid}/",
                route="/api/workspaces/{slug}/user-favorites/{pk}/",
                auth="admin", expect_status=204,
            ),
            # Quick links: create -> list -> patch -> missing-url
            # 400 -> delete -> 404.
            Request(
                "POST", _WS + "/quick-links/",
                route="/api/workspaces/{slug}/quick-links/",
                json={"title": "Q1", "url": "https://example.com/x"},
                auth="admin",
                capture={"qid": "id"},
                expect_status=201,
            ),
            Request(
                "GET", _WS + "/quick-links/",
                route="/api/workspaces/{slug}/quick-links/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _WS + "/quick-links/{qid}/",
                route="/api/workspaces/{slug}/quick-links/{pk}/",
                json={"title": "Q2"},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _WS + "/quick-links/",
                route="/api/workspaces/{slug}/quick-links/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "DELETE", _WS + "/quick-links/{qid}/",
                route="/api/workspaces/{slug}/quick-links/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", _WS + "/quick-links/{qid}/",
                route="/api/workspaces/{slug}/quick-links/{pk}/",
                auth="admin", expect_status=404,
            ),
            # Home prefs: autocreate-read -> patch -> unknown-key
            # 400 (watch: suite pins capital-D Detail) -> patch the
            # bare route (view requires key; empirical) -> verify.
            Request(
                "GET", _WS + "/home-preferences/",
                route="/api/workspaces/{slug}/home-preferences/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _WS + "/home-preferences/recents/",
                route="/api/workspaces/{slug}/home-preferences/{key}/",
                json={"is_enabled": False},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _WS + "/home-preferences/nope/",
                route="/api/workspaces/{slug}/home-preferences/{key}/",
                json={"is_enabled": False},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _WS + "/home-preferences/",
                route="/api/workspaces/{slug}/home-preferences/",
                json={"is_enabled": False},
                auth="admin", expect_status=500,
            ),
            Request(
                "GET", _WS + "/home-preferences/",
                route="/api/workspaces/{slug}/home-preferences/",
                auth="admin", expect_status=200,
            ),
            # Sidebar prefs: read -> patch -> verify.
            Request(
                "GET", _WS + "/sidebar-preferences/",
                route="/api/workspaces/{slug}/sidebar-preferences/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _WS + "/sidebar-preferences/",
                route="/api/workspaces/{slug}/sidebar-preferences/",
                json=[{"key": "views", "is_pinned": True}],
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _WS + "/sidebar-preferences/",
                route="/api/workspaces/{slug}/sidebar-preferences/",
                auth="admin", expect_status=200,
            ),
            # User props: read (autocreate) -> patch -> verify.
            Request(
                "GET", _WS + "/user-properties/",
                route="/api/workspaces/{slug}/user-properties/",
                auth="admin",
                capture={"uprops": "id"},
                expect_status=200,
            ),
            Request(
                "PATCH", _WS + "/user-properties/",
                route="/api/workspaces/{slug}/user-properties/",
                json={"navigation_project_limit": 7},
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _WS + "/user-properties/",
                route="/api/workspaces/{slug}/user-properties/",
                auth="admin", expect_status=200,
            ),
            # Workspace views save (204) + activity export (CSV).
            Request(
                "POST", _WS + "/workspace-views/",
                route="/api/workspaces/{slug}/workspace-views/",
                json={"view_props": {"layout": "kanban"}},
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", _WS + "/user-activity/{admin}/export/",
                route="/api/workspaces/{slug}/user-activity/{pk}/export/",
                json={"date": "2026-09-24"},
                auth="admin", expect_status=200,
            ),
            # Stickies: create -> list -> patch -> delete -> 404.
            Request(
                "POST", _WS + "/stickies/",
                route="/api/workspaces/{slug}/stickies/",
                json={"name": "S1"},
                auth="admin",
                capture={"sid": "id", "sbg": "background_color"},
                expect_status=201,
            ),
            Request(
                "GET", _WS + "/stickies/",
                route="/api/workspaces/{slug}/stickies/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _WS + "/stickies/{sid}/",
                route="/api/workspaces/{slug}/stickies/{pk}/",
                json={"name": "S2"},
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _WS + "/stickies/{sid}/",
                route="/api/workspaces/{slug}/stickies/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", _WS + "/stickies/{sid}/",
                route="/api/workspaces/{slug}/stickies/{pk}/",
                auth="admin", expect_status=404,
                # PIDASHCONV-907 is Done (bfc6e8b0): Rust renders
                # Django's lowercase detail + model message here.
            ),
        ),
    ),
)
