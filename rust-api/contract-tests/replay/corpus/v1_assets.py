# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: v1 assets / stickies / runner-delete write flows (PIDASHCONV-823).

Built from the ``api_assets_stickies_intake`` contract suite
(test_user_assets.py, test_generic_assets.py, test_stickies.py) and
``v1_cli_auth/test_runner_delete.py``. API-key auth throughout (the
suites' admin harness), so the 868 process_logs pin applies.
"""

from psycopg.rows import dict_row

from ..session import Request, Session

# Fixed ids seeded by _seed_extras (addressed via static_context).
POD_ID = "99999999-0000-4000-8000-000000000001"
RUN_ADMIN_ID = "99999999-0000-4000-8000-000000000002"
RUN_MEMBER_ID = "99999999-0000-4000-8000-000000000003"
STICKY_MEMBER_ID = "99999999-0000-4000-8000-000000000004"
UASSET_MEMBER_ID = "99999999-0000-4000-8000-000000000005"

_USER = "/api/v1/assets/user-assets/"
_SERVER = "/api/v1/assets/user-assets/server/"
_WS_ASSETS = "/api/v1/workspaces/{ws}/assets"
_STICKIES = "/api/v1/workspaces/{ws}/stickies"
_RUNNERS = "/api/v1/runners"

_UPLOAD = {"name": "profile.jpg", "type": "image/jpeg", "size": 1024000,
           "entity_type": "USER_AVATAR"}
_GUPLOAD = {"name": "report.pdf", "type": "application/pdf", "size": 204800}


def _seed_extras(conn) -> None:
    """Pod + runners + member-owned sticky and user asset (fixed ids).

    The pod mirrors ``_harness.seed.Seeder.pod`` (same values, fixed
    id); the runners mirror ``Seeder.runner`` (offline, visibility 0
    = private, owner-scoped delete); the sticky mirrors the suite's
    create shape; the user asset mirrors a minted USER_AVATAR row
    owned by member (for the cross-user 404 legs).
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM projects WHERE identifier = 'RPJ'")
        project_id = str(cur.fetchone()["id"])
        cur.execute(
            "SELECT id FROM users WHERE email = 'replay-admin@example.com'"
        )
        admin_id = str(cur.fetchone()["id"])
        cur.execute(
            "SELECT id FROM users WHERE email = 'replay-member@example.com'"
        )
        member_id = str(cur.fetchone()["id"])
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO pod (id, name, description, is_default, workspace_id,
                project_id, created_at, updated_at)
            VALUES (%s, 'rp-asset-pod', '', false, %s, %s, now(), now())
            """,
            (POD_ID, ws_id, project_id),
        )
        for run_id, owner_id, name in (
            (RUN_ADMIN_ID, admin_id, "rp-own"),
            (RUN_MEMBER_ID, member_id, "rp-alien"),
        ):
            cur.execute(
                """
                INSERT INTO runner (id, name, capabilities, status, os, arch,
                    runner_version, protocol_version, created_at, updated_at,
                    owner_id, workspace_id, pod_id, host_label,
                    refresh_token_hash, refresh_token_fingerprint,
                    refresh_token_generation, previous_refresh_token_hash,
                    access_token_signing_key_version, enrollment_token_hash,
                    enrollment_token_fingerprint, revoked_reason, visibility,
                    dev_metadata, provisioning)
                VALUES (%s, %s, '[]', 'offline', '', '', '', 1, now(), now(),
                    %s, %s, %s, '', '', '', 0, '', 1, '', '', '', 0, '{}',
                    'manual')
                """,
                (run_id, name, owner_id, ws_id, POD_ID),
            )
        cur.execute(
            """
            INSERT INTO stickies (id, name, description, description_html,
                description_stripped, logo_props, owner_id, created_by_id,
                workspace_id, sort_order, created_at, updated_at)
            VALUES (%s, 'member-note', '{}', '<p>member words</p>',
                'member words', '{}', %s, %s, %s, 65535.0, now(), now())
            """,
            (STICKY_MEMBER_ID, member_id, member_id, ws_id),
        )
        cur.execute(
            """
            INSERT INTO file_assets (id, attributes, asset, created_by_id,
                user_id, workspace_id, is_deleted, is_archived, is_uploaded,
                entity_type, size, created_at, updated_at)
            VALUES (%s, '{}', 'member/avatar.jpg', %s, %s, %s, false, false,
                false, 'USER_AVATAR', 1024000, now(), now())
            """,
            (UASSET_MEMBER_ID, member_id, member_id, ws_id),
        )
    conn.commit()


# The mint responses embed per-backend randomness (space session's
# presign pattern): asset_url's trailing id, the presigned key (an
# independent random uuid plus filename), the policy + signature
# derived from it, and the SigV4 mint instant (x-amz-date is
# per-second — mints straddling a second boundary flaked the exact
# compare — while x-amz-credential embeds the date, a UTC-midnight
# flake). Presence-asserted, values blanked; every other leaf
# (Content-Type, algorithm, url) still compares exactly. The stored
# file_assets.asset key is the same value the response returns, so
# the asset_key capture also substitutes the DB cell on each side.
_PRESIGN_BLANKS = [
    "asset_url",
    "upload_data/fields/key",
    "upload_data/fields/policy",
    "upload_data/fields/x-amz-signature",
    "upload_data/fields/x-amz-date",
    "upload_data/fields/x-amz-credential",
]


def _sticky_404_pin():
    """Exact-shape pin for PIDASHCONV-899 (sticky detail 404 legs).

    Django renders DRF's get_object_or_404 message ("No Sticky
    matches the given query."); Rust renders the bare Http404 shape
    ("Not found."). Any byte change on either side fails loudly.
    """
    return [
        ("detail", "PIDASHCONV-899",
         "No Sticky matches the given query.", "Not found."),
    ]


SESSIONS = (
    Session(
        name="v1_assets_lifecycle",
        domain="v1_assets",
        source="api_assets_stickies_intake/test_user_assets.py, "
        "test_generic_assets.py, test_stickies.py; "
        "v1_cli_auth/test_runner_delete.py",
        auth_users=("api_key",),
        seed=_seed_extras,
        static_context={
            "pod1": POD_ID,
            "run_own": RUN_ADMIN_ID,
            "run_alien": RUN_MEMBER_ID,
            "sticky_alien": STICKY_MEMBER_ID,
            "uasset_alien": UASSET_MEMBER_ID,
        },
        payload_time_paths={
            "pi_dash.bgtasks.issue_activities_task.issue_activity": ["kwargs/epoch"],
        },
        # Known divergence: Rust never enqueues the API-token audit log
        # (PIDASHCONV-868, shared with the other v1 sessions).
        known_absent_jobs={
            "pi_dash.bgtasks.logger_task.process_logs": "PIDASHCONV-868",
        },
        requests=(
            # User assets: mint -> invalid-entity 400 -> invalid-type
            # 400 -> patch -> unknown 404 -> alien 404 -> delete ->
            # delete-again 404.
            Request(
                "POST", _USER,
                route="/api/v1/assets/user-assets/",
                json=dict(_UPLOAD),
                auth="api_key",
                capture={"ua": "asset_id",
                         "ua_key": "upload_data/fields/key"},
                body_blank_paths=list(_PRESIGN_BLANKS),
                expect_status=200,
            ),
            Request(
                "POST", _USER,
                route="/api/v1/assets/user-assets/",
                json={**_UPLOAD, "entity_type": "ISSUE_ATTACHMENT"},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", _USER,
                route="/api/v1/assets/user-assets/",
                json={**_UPLOAD, "type": "application/pdf"},
                auth="api_key", expect_status=400,
            ),
            Request(
                "PATCH", _USER + "{ua}/",
                route="/api/v1/assets/user-assets/{pk}/",
                json={"attributes": {"caption": "me"}},
                auth="api_key", expect_status=204,
            ),
            Request(
                "PATCH", _USER + "123e4567-e89b-12d3-a456-426614174000/",
                route="/api/v1/assets/user-assets/{pk}/",
                json={"attributes": {}},
                auth="api_key", expect_status=404,
            ),
            Request(
                "PATCH", _USER + "{uasset_alien}/",
                route="/api/v1/assets/user-assets/{pk}/",
                json={"attributes": {}},
                auth="api_key", expect_status=404,
            ),
            Request(
                "DELETE", _USER + "{ua}/",
                route="/api/v1/assets/user-assets/{pk}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _USER + "{ua}/",
                route="/api/v1/assets/user-assets/{pk}/",
                auth="api_key", expect_status=404,
            ),
            # Server variant: mint -> invalid-entity 400 -> patch ->
            # delete.
            Request(
                "POST", _SERVER,
                route="/api/v1/assets/user-assets/server/",
                json=dict(_UPLOAD),
                auth="api_key",
                capture={"sa": "asset_id",
                         "sa_key": "upload_data/fields/key"},
                body_blank_paths=list(_PRESIGN_BLANKS),
                expect_status=200,
            ),
            Request(
                "POST", _SERVER,
                route="/api/v1/assets/user-assets/server/",
                json={**_UPLOAD, "entity_type": "NOPE"},
                auth="api_key", expect_status=400,
            ),
            Request(
                "PATCH", _USER + "{sa}/server/",
                route="/api/v1/assets/user-assets/{pk}/server/",
                json={"attributes": {}},
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _USER + "{sa}/server/",
                route="/api/v1/assets/user-assets/{pk}/server/",
                auth="api_key", expect_status=204,
            ),
            # Generic workspace assets: mint -> project/external mint
            # -> external-conflict 409 -> missing-name 400 ->
            # invalid-type 400 -> patch (mark uploaded) -> download
            # 200 -> unknown-asset 404.
            Request(
                "POST", _WS_ASSETS + "/",
                route="/api/v1/workspaces/{slug}/assets/",
                json=dict(_GUPLOAD),
                auth="api_key",
                capture={"ga": "asset_id",
                         "ga_key": "upload_data/fields/key"},
                body_blank_paths=list(_PRESIGN_BLANKS),
                expect_status=200,
            ),
            Request(
                "POST", _WS_ASSETS + "/",
                route="/api/v1/workspaces/{slug}/assets/",
                json={**_GUPLOAD, "project_id": "{project}",
                      "external_id": "ext-123",
                      "external_source": "github"},
                auth="api_key",
                capture={"ga2": "asset_id",
                         "ga2_key": "upload_data/fields/key"},
                body_blank_paths=list(_PRESIGN_BLANKS),
                expect_status=200,
            ),
            Request(
                "POST", _WS_ASSETS + "/",
                route="/api/v1/workspaces/{slug}/assets/",
                json={"name": "other.pdf", "type": "application/pdf",
                      "size": 100, "external_id": "ext-123",
                      "external_source": "github"},
                auth="api_key",
                # The 409 echoes the existing row's url, whose
                # trailing id is backend-local (the ga2 capture only
                # substitutes whole values).
                body_blank_paths=["asset_url"],
                expect_status=409,
            ),
            Request(
                "POST", _WS_ASSETS + "/",
                route="/api/v1/workspaces/{slug}/assets/",
                json={"type": "application/pdf"},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", _WS_ASSETS + "/",
                route="/api/v1/workspaces/{slug}/assets/",
                json={"name": "x.bin", "type": "application/x-sh",
                      "size": 100},
                auth="api_key", expect_status=400,
            ),
            Request(
                "PATCH", _WS_ASSETS + "/{ga}/",
                route="/api/v1/workspaces/{slug}/assets/{pk}/",
                json={"is_uploaded": True},
                auth="api_key", expect_status=204,
            ),
            Request(
                "GET", _WS_ASSETS + "/{ga}/",
                route="/api/v1/workspaces/{slug}/assets/{pk}/",
                auth="api_key",
                body_blank_paths=["asset_url"],
                expect_status=200,
            ),
            Request(
                "GET", _WS_ASSETS + "/123e4567-e89b-12d3-a456-426614174000/",
                route="/api/v1/workspaces/{slug}/assets/{pk}/",
                auth="api_key", expect_status=404,
            ),
            # Stickies: create -> patch -> unknown 404 -> alien
            # patch/delete 404s -> delete -> delete-again 404.
            Request(
                "POST", _STICKIES + "/",
                route="/api/v1/workspaces/{slug}/stickies/",
                json={"name": "note",
                      "description_html": "<p>hello world</p>"},
                auth="api_key", capture={"st": "id"},
                expect_status=201,
            ),
            Request(
                "PATCH", _STICKIES + "/{st}/",
                route="/api/v1/workspaces/{slug}/stickies/{pk}/",
                json={"name": "renamed", "color": "#ff0000"},
                auth="api_key", expect_status=200,
            ),
            Request(
                "PATCH", _STICKIES + "/123e4567-e89b-12d3-a456-426614174000/",
                route="/api/v1/workspaces/{slug}/stickies/{pk}/",
                json={"name": "nope"},
                auth="api_key", expect_status=404,
                known_body_diffs=_sticky_404_pin(),
            ),
            Request(
                "PATCH", _STICKIES + "/{sticky_alien}/",
                route="/api/v1/workspaces/{slug}/stickies/{pk}/",
                json={"name": "nope"},
                auth="api_key", expect_status=404,
                known_body_diffs=_sticky_404_pin(),
            ),
            Request(
                "DELETE", _STICKIES + "/{sticky_alien}/",
                route="/api/v1/workspaces/{slug}/stickies/{pk}/",
                auth="api_key", expect_status=404,
                known_body_diffs=_sticky_404_pin(),
            ),
            Request(
                "DELETE", _STICKIES + "/{st}/",
                route="/api/v1/workspaces/{slug}/stickies/{pk}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _STICKIES + "/{st}/",
                route="/api/v1/workspaces/{slug}/stickies/{pk}/",
                auth="api_key", expect_status=404,
                known_body_diffs=_sticky_404_pin(),
            ),
            # Runners: unknown 404 -> alien 404 -> bad purge-flag 400
            # -> delete -> delete-again 404. (No anonymous 401 leg:
            # Django skips process_logs for unauthenticated requests,
            # which would trip the session-level 868 pin; the
            # contract suite covers the 401 shape.)
            Request(
                "DELETE", _RUNNERS + "/123e4567-e89b-12d3-a456-426614174000/",
                route="/api/v1/runners/{pk}/",
                auth="api_key", expect_status=404,
            ),
            Request(
                "DELETE", _RUNNERS + "/{run_alien}/",
                route="/api/v1/runners/{pk}/",
                auth="api_key", expect_status=404,
            ),
            Request(
                "DELETE", _RUNNERS + "/{run_own}/",
                route="/api/v1/runners/{pk}/",
                params={"purge_local": "maybe"},
                auth="api_key", expect_status=400,
            ),
            Request(
                "DELETE", _RUNNERS + "/{run_own}/",
                route="/api/v1/runners/{pk}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _RUNNERS + "/{run_own}/",
                route="/api/v1/runners/{pk}/",
                auth="api_key", expect_status=404,
            ),
        ),
    ),
)
