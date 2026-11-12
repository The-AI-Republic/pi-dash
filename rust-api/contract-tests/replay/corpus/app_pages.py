# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app-tier page write flows (PIDASHCONV-823).

Built from the ``app_pages`` contract suite: test_pages.py
(CRUD + summary), test_page_actions.py (favorites, archive,
lock, access), test_page_description.py (description PATCH),
test_page_versions.py (duplicate). Session-cookie auth
(admin/member/guest), like the suite.
"""

from psycopg.rows import dict_row

from ..session import Request, Session

# Fixed page ids seeded by _seed_pages (addressed via static_context).
PG_PARENT_ID = "aaaaaaaa-0000-4000-8000-000000000001"
PG_CHILD_ID = "aaaaaaaa-0000-4000-8000-000000000002"
PG_LOCKED_ID = "aaaaaaaa-0000-4000-8000-000000000003"
PG_ARCH_ID = "aaaaaaaa-0000-4000-8000-000000000004"
PG_UNLINKED_ID = "aaaaaaaa-0000-4000-8000-000000000005"
PG_ALIEN_ID = "aaaaaaaa-0000-4000-8000-000000000006"
BR_PARENT_ID = "aaaaaaaa-0000-4000-8000-000000000011"
BR_CHILD_ID = "aaaaaaaa-0000-4000-8000-000000000012"
BR_LOCKED_ID = "aaaaaaaa-0000-4000-8000-000000000013"
BR_ARCH_ID = "aaaaaaaa-0000-4000-8000-000000000014"
BR_ALIEN_ID = "aaaaaaaa-0000-4000-8000-000000000015"

_P = "/api/workspaces/{ws}/projects/{project}"


def _seed_pages(conn) -> None:
    """Parent/child + locked + archived + unlinked + alien pages.

    The page INSERT mirrors ``_harness.seed.create_page`` (same
    columns); the bridge INSERT mirrors ``link_page_project``.
    PG_UNLINKED has no bridge (owner-error legs); PG_ALIEN lives in
    replay-other (cross-workspace isolation leg).
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-other'")
        other_ws_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM projects WHERE identifier = 'RPJ'")
        project_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM projects WHERE identifier = 'RPX'")
        other_project_id = str(cur.fetchone()["id"])
        cur.execute(
            "SELECT email, id FROM users WHERE email LIKE 'replay-%%@example.com'"
        )
        users = {r["email"]: str(r["id"]) for r in cur.fetchall()}
    admin_id = users["replay-admin@example.com"]
    outsider_id = users["replay-outsider@example.com"]
    with conn.cursor() as cur:
        for page_id, name, owner, ws, parent, archived, locked in (
            (PG_PARENT_ID, "Parent", admin_id, ws_id, None, False, False),
            (PG_CHILD_ID, "Child", admin_id, ws_id, PG_PARENT_ID, False, False),
            (PG_LOCKED_ID, "Locked", admin_id, ws_id, None, False, True),
            (PG_ARCH_ID, "Archived", admin_id, ws_id, None, True, False),
            (PG_UNLINKED_ID, "Elsewhere", admin_id, ws_id, None, False, False),
            (PG_ALIEN_ID, "Alien", outsider_id, other_ws_id, None, False, False),
        ):
            cur.execute(
                """
                INSERT INTO pages
                   (id, name, description_json, description_binary, description_html,
                    owned_by_id, access, workspace_id, color, parent_id, archived_at,
                    is_locked, view_props, logo_props, is_global, sort_order,
                    created_by_id, created_at, updated_at)
                VALUES (%s, %s, '{}', NULL, '<p></p>', %s, 0, %s, '', %s,
                    CASE WHEN %s THEN CURRENT_DATE ELSE NULL END,
                    %s, '{"full_width": false}', '{}', false, 65535, %s, now(), now())
                """,
                (page_id, name, owner, ws, parent, archived, locked, owner),
            )
        for bridge_id, ws, project, page, by_id in (
            (BR_PARENT_ID, ws_id, project_id, PG_PARENT_ID, admin_id),
            (BR_CHILD_ID, ws_id, project_id, PG_CHILD_ID, admin_id),
            (BR_LOCKED_ID, ws_id, project_id, PG_LOCKED_ID, admin_id),
            (BR_ARCH_ID, ws_id, project_id, PG_ARCH_ID, admin_id),
            (BR_ALIEN_ID, other_ws_id, other_project_id, PG_ALIEN_ID, outsider_id),
        ):
            cur.execute(
                """
                INSERT INTO project_pages
                   (id, workspace_id, project_id, page_id, created_by_id, updated_by_id,
                    deleted_at, created_at, updated_at)
                VALUES (%s, %s, %s, %s, %s, %s, NULL, now(), now())
                """,
                (bridge_id, ws, project, page, by_id, by_id),
            )
    conn.commit()


SESSIONS = (
    Session(
        name="app_pages_lifecycle",
        domain="app_pages",
        source="app_pages/test_pages.py, test_page_actions.py, "
        "test_page_description.py, test_page_versions.py",
        auth_users=("admin", "member", "guest"),
        seed=_seed_pages,
        static_context={
            "parent": PG_PARENT_ID,
            "child": PG_CHILD_ID,
            "locked": PG_LOCKED_ID,
            "arch": PG_ARCH_ID,
            "unlinked": PG_UNLINKED_ID,
            "alien": PG_ALIEN_ID,
        },
        requests=(
            # CRUD: create -> list -> retrieve -> patch ->
            # denied/edge patches -> destroy-must-archive 400.
            Request(
                "POST", _P + "/pages/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/",
                json={"name": "P1"},
                auth="admin",
                capture={"p1": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _P + "/pages/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/",
                json={"name": "Guest page"},
                auth="guest", expect_status=403,
            ),
            Request(
                "GET", _P + "/pages/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _P + "/pages/{p1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _P + "/pages/{p1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/",
                json={"name": "P1b"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _P + "/pages/{unlinked}/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/",
                json={"name": "Ghost"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _P + "/pages/{locked}/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/",
                json={"name": "Nope"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _P + "/pages/{p1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/",
                json={"access": 1},
                auth="member", expect_status=400,
            ),
            Request(
                "DELETE", _P + "/pages/{p1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/",
                auth="admin", expect_status=400,
            ),
            # Favorites: create -> destroy.
            Request(
                "POST", _P + "/favorite-pages/{p1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/"
                "favorite-pages/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _P + "/favorite-pages/{p1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/"
                "favorite-pages/{pk}/",
                auth="admin", expect_status=204,
            ),
            # Access: set -> member-denied 400.
            Request(
                "POST", _P + "/pages/{p1}/access/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/access/",
                json={"access": 1},
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", _P + "/pages/{child}/access/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/access/",
                json={"access": 1},
                auth="member", expect_status=400,
            ),
            # Description: html -> json -> locked 400 -> archived
            # 400 -> binary-read (empty).
            Request(
                "PATCH", _P + "/pages/{p1}/description/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/"
                "description/",
                json={"description_html": "<p>hi</p>"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _P + "/pages/{p1}/description/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/"
                "description/",
                json={"description_json": {"ops": []}},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _P + "/pages/{locked}/description/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/"
                "description/",
                json={"description_html": "<p>x</p>"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _P + "/pages/{arch}/description/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/"
                "description/",
                json={"description_html": "<p>x</p>"},
                auth="admin", expect_status=400,
            ),
            Request(
                "GET", _P + "/pages/{p1}/description/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/"
                "description/",
                auth="admin", expect_status=200,
            ),
            # Lock: lock -> patch-denied -> unlock.
            Request(
                "POST", _P + "/pages/{p1}/lock/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/lock/",
                auth="admin", expect_status=204,
            ),
            Request(
                "PATCH", _P + "/pages/{p1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/",
                json={"name": "Nope"},
                auth="admin", expect_status=400,
            ),
            Request(
                "DELETE", _P + "/pages/{p1}/lock/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/lock/",
                auth="admin", expect_status=204,
            ),
            # Duplicate the parent, then archive the parent (covers
            # the child), destroy it, and unarchive the child (the
            # archived parent detaches).
            Request(
                "POST", _P + "/pages/{parent}/duplicate/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/"
                "duplicate/",
                auth="admin",
                capture={"pdup": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _P + "/pages/{parent}/archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/"
                "archive/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _P + "/pages/{pdup}/archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/"
                "archive/",
                auth="member", expect_status=400,
            ),
            Request(
                "DELETE", _P + "/pages/{parent}/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/",
                auth="admin", expect_status=204,
                # PIDASHCONV-908 is Done (7cc7e1dd): Rust's
                # soft-delete recomputes description_stripped
                # like Page.save(), so both sides agree here.
            ),
            Request(
                "GET", _P + "/pages/{parent}/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "DELETE", _P + "/pages/{child}/archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/"
                "archive/",
                auth="admin", expect_status=204,
            ),
            # Member destroy tripwire on the archived seed page,
            # then plain unarchive; cross-workspace lock 404.
            Request(
                "DELETE", _P + "/pages/{arch}/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/",
                auth="member", expect_status=403,
            ),
            Request(
                "DELETE", _P + "/pages/{arch}/archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/"
                "archive/",
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", _P + "/pages/{alien}/lock/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/lock/",
                auth="admin", expect_status=404,
            ),
            # Close out P1: archive -> destroy -> 404.
            Request(
                "POST", _P + "/pages/{p1}/archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/"
                "archive/",
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _P + "/pages/{p1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/",
                auth="admin", expect_status=204,
                # No 908 pin here: P1's stripped text was honestly
                # recomputed to 'hi' by the description PATCH on
                # both sides, so the DELETE rows agree.
            ),
            Request(
                "GET", _P + "/pages/{p1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/pages/{pk}/",
                auth="admin", expect_status=404,
            ),
        ),
    ),
)
