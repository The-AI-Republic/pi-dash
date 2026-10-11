# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app-tier views + favorites write flows (PIDASHCONV-823).

Built from the ``app_views_search`` contract suite:
test_project_views.py (project views CRUD, locked/non-owner guards,
favorites marking) and test_global_views.py (global views CRUD,
member destroy, favorite create/delete, the broken favorites-list
500). Session-cookie auth (admin/member/guest), like the suite.
"""

from psycopg.rows import dict_row

from ..session import Request, Session

# Fixed view ids seeded by _seed_views (addressed via static_context).
LOCKED_ID = "66666666-0000-4000-8000-000000000001"
MEMBER_GLOBAL_ID = "66666666-0000-4000-8000-000000000002"

_P = "/api/workspaces/{ws}/projects/{project}/views"
_F = "/api/workspaces/{ws}/projects/{project}/user-favorite-views"
_G = "/api/workspaces/{ws}/views"


def _seed_views(conn) -> None:
    """A locked project view (PATCH-400 leg) and a member-owned global
    view (member-destroy-204 leg).

    The INSERT mirrors ``_harness.seed.Seeder.issue_view`` (same
    columns), with created_by mirroring owned_by the way API-created
    rows look.
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM projects WHERE identifier = 'RPJ'")
        project_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM users WHERE email = 'replay-admin@example.com'")
        admin_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM users WHERE email = 'replay-member@example.com'")
        member_id = str(cur.fetchone()["id"])
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO issue_views
               (id, name, description, query, filters, display_filters,
                display_properties, logo_props, rich_filters, access,
                sort_order, is_locked, owned_by_id, created_by_id,
                workspace_id, project_id, created_at, updated_at)
            VALUES (%s, 'Locked', '', '{}', '{}', '{}', '{}', '{}', '{}',
                    1, 65535, true, %s, %s, %s, %s, now(), now())
            """,
            (LOCKED_ID, admin_id, admin_id, ws_id, project_id),
        )
        cur.execute(
            """
            INSERT INTO issue_views
               (id, name, description, query, filters, display_filters,
                display_properties, logo_props, rich_filters, access,
                sort_order, is_locked, owned_by_id, created_by_id,
                workspace_id, project_id, created_at, updated_at)
            VALUES (%s, 'Member global', '', '{}', '{}', '{}', '{}', '{}', '{}',
                    1, 65535, false, %s, %s, %s, NULL, now(), now())
            """,
            (MEMBER_GLOBAL_ID, member_id, member_id, ws_id),
        )
    conn.commit()


SESSIONS = (
    Session(
        name="app_views_lifecycle",
        domain="app_views_search",
        source="app_views_search/test_project_views.py, test_global_views.py",
        auth_users=("admin", "member", "guest"),
        seed=_seed_views,
        static_context={
            "locked": LOCKED_ID,
            "member_global": MEMBER_GLOBAL_ID,
        },
        requests=(
            # Project views: create -> list -> detail -> patch ->
            # put -> locked 400 -> favorite 204 -> list shows the
            # star -> broken favorites-list 500 -> unfavorite.
            Request(
                "POST", _P + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/views/",
                json={"name": "Sprint board", "filters": {}},
                auth="admin",
                capture={"pv": "id"},
                expect_status=201,
            ),
            Request(
                "GET", _P + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/views/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _P + "/{pv}/",
                route="/api/workspaces/{slug}/projects/{project_id}/views/{pk}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _P + "/{pv}/",
                route="/api/workspaces/{slug}/projects/{project_id}/views/{pk}/",
                json={"name": "After"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PUT", _P + "/{pv}/",
                route="/api/workspaces/{slug}/projects/{project_id}/views/{pk}/",
                json={"name": "Put done", "filters": {}},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _P + "/{locked}/",
                route="/api/workspaces/{slug}/projects/{project_id}/views/{pk}/",
                json={"name": "Unlock attempt"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _F + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/user-favorite-views/",
                json={"view": "{pv}"},
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", _P + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/views/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _F + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/user-favorite-views/",
                auth="admin", expect_status=500,
            ),
            Request(
                "DELETE", _F + "/{pv}/",
                route="/api/workspaces/{slug}/projects/{project_id}/user-favorite-views/{view_id}/",
                auth="admin", expect_status=204,
            ),
            # Project view deletes: member non-owner 403, then the
            # owner 204, then the list no longer shows it.
            Request(
                "DELETE", _P + "/{pv}/",
                route="/api/workspaces/{slug}/projects/{project_id}/views/{pk}/",
                auth="member", expect_status=403,
            ),
            Request(
                "DELETE", _P + "/{pv}/",
                route="/api/workspaces/{slug}/projects/{project_id}/views/{pk}/",
                auth="admin", expect_status=204,
                # Known divergence (PIDASHCONV-923): Rust's
                # project_destroy soft-deletes with no enqueue;
                # Django's model delete() enqueues
                # soft_delete_related_objects.
                known_absent_jobs={
                    "pi_dash.bgtasks.deletion_task.soft_delete_related_objects": "PIDASHCONV-923",
                },
            ),
            Request(
                "GET", _P + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/views/",
                auth="admin", expect_status=200,
            ),
            # Global views: create -> list -> detail -> patch ->
            # put -> guest sees only own (none) -> member
            # non-owner 403 -> member destroys own -> owner 204.
            Request(
                "POST", _G + "/",
                route="/api/workspaces/{slug}/views/",
                json={"name": "Global board", "filters": {}},
                auth="admin",
                capture={"gv": "id"},
                expect_status=201,
            ),
            Request(
                "GET", _G + "/",
                route="/api/workspaces/{slug}/views/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _G + "/{gv}/",
                route="/api/workspaces/{slug}/views/{pk}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _G + "/{gv}/",
                route="/api/workspaces/{slug}/views/{pk}/",
                json={"name": "G-after"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PUT", _G + "/{gv}/",
                route="/api/workspaces/{slug}/views/{pk}/",
                json={"name": "G-put", "filters": {}},
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _G + "/",
                route="/api/workspaces/{slug}/views/",
                auth="guest", expect_status=200,
            ),
            Request(
                "DELETE", _G + "/{gv}/",
                route="/api/workspaces/{slug}/views/{pk}/",
                auth="member", expect_status=403,
            ),
            Request(
                "DELETE", _G + "/{member_global}/",
                route="/api/workspaces/{slug}/views/{pk}/",
                auth="member", expect_status=204,
                # Known divergence (PIDASHCONV-925): Django's soft
                # delete stamps updated_by like the model save;
                # Rust's workspace_destroy only stamps deleted_at.
                known_cell_diffs=[
                    ("issue_views", "PIDASHCONV-925",
                     {"name": "Member global"}, "updated_by_id",
                     "{member}", None),
                ],
                # Known divergence (PIDASHCONV-924): Rust's
                # workspace_destroy soft-deletes with no enqueue;
                # Django's model delete() enqueues
                # soft_delete_related_objects.
                known_absent_jobs={
                    "pi_dash.bgtasks.deletion_task.soft_delete_related_objects": "PIDASHCONV-924",
                },
            ),
            Request(
                "DELETE", _G + "/{gv}/",
                route="/api/workspaces/{slug}/views/{pk}/",
                auth="admin", expect_status=204,
                # Known divergence (PIDASHCONV-924): same missing
                # soft_delete enqueue through workspace_destroy
                # (the gv row was PUT-updated, so the 925 stamping
                # gap is invisible here — updated_by is already
                # the deleter on both sides).
                known_absent_jobs={
                    "pi_dash.bgtasks.deletion_task.soft_delete_related_objects": "PIDASHCONV-924",
                },
            ),
            Request(
                "GET", _G + "/",
                route="/api/workspaces/{slug}/views/",
                auth="admin", expect_status=200,
            ),
            # The lock guard covers updates, not destroy: the seeded
            # locked view deletes 204 through the project route (and
            # proves project_destroy shares the 925 stamping gap —
            # the row was never updated, so updated_by is None until
            # the delete stamps it).
            Request(
                "DELETE", _P + "/{locked}/",
                route="/api/workspaces/{slug}/projects/{project_id}/views/{pk}/",
                auth="admin", expect_status=204,
                known_absent_jobs={
                    "pi_dash.bgtasks.deletion_task.soft_delete_related_objects": "PIDASHCONV-923",
                },
                # Known divergence (PIDASHCONV-925): Django's soft
                # delete stamps updated_by like the model save;
                # Rust's project_destroy only stamps deleted_at.
                known_cell_diffs=[
                    ("issue_views", "PIDASHCONV-925",
                     {"name": "Locked"}, "updated_by_id",
                     "{admin}", None),
                ],
            ),
            Request(
                "GET", _P + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/views/",
                auth="admin", expect_status=200,
            ),
        ),
    ),
)
