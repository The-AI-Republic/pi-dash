# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app-tier asset write flows (PIDASHCONV-823).

Built from the ``app_assets`` contract suite: test_user_assets_v2.py,
test_workspace_assets.py, test_project_assets.py, test_duplicate.py,
test_static_restore_check.py and test_legacy_v1.py. Session-cookie auth
(admin/member/guest/outsider), like the suite.

Two sessions: ``app_assets_lifecycle`` (v2 presigned mints, confirms,
deletes, bulk attach, restores) and ``app_assets_legacy`` (v1
file-assets incl. the ported storage/serializer 500s). Mint responses
embed per-backend randomness (presigned key/policy/signature/SigV4
instant); the v1_assets presign pattern applies (presence-asserted,
values blanked, stored key aligned through the key capture).

Deliberately uncovered behaviors (routes still covered via error legs):
the duplicate-success leg (``DuplicateAssetEndpoint`` copies through
live S3; the suite pins 200-or-500 and the clone key is unknowable on a
500, so no exact compare is possible — invalid-entity/unknown/denied
legs cover the pair) and the 302 redirect GETs (per-backend presigned
Location; the suite pins the shape).
"""

from psycopg.rows import dict_row

from ..session import Request, Session

# Fixed file_assets ids/keys seeded by _seed_legacy (static_context).
WS_ROW_ID = "a5a5a5a5-0000-4000-8000-000000000001"
WS_ROW_LEAF = "a5seed01-back.txt"
USER_ROW_ID = "a5a5a5a5-0000-4000-8000-000000000002"
USER_ROW_KEY = "user-a5seed01-mine.txt"

MISSING_ID = "00000000-0000-0000-0000-000000000000"

_U = "/api/assets/v2/user-assets"
_W = "/api/assets/v2/workspaces/{ws}"
_P = "/api/assets/v2/workspaces/{ws}/projects/{project}"

# Mint-response presign blanks (v1_assets pattern): asset_url's
# trailing id, the presigned key (independent random hex + filename),
# the policy + signature derived from it, and the SigV4 mint instant.
# Every other leaf (Content-Type, algorithm, url) still compares, and
# the key capture substitutes the stored file_assets.asset cell.
_PRESIGN_BLANKS = [
    "asset_url",
    "upload_data/fields/key",
    "upload_data/fields/policy",
    "upload_data/fields/x-amz-signature",
    "upload_data/fields/x-amz-date",
    "upload_data/fields/x-amz-credential",
]


def _seed_legacy(conn) -> None:
    """One uploaded workspace file_assets row (fixed id + key) and one
    uploaded user row (fixed id + key) for the v1 legs.

    The INSERT mirrors ``app_assets/seed_assets.py::create_asset``
    (same columns); keys use fixed hex instead of random (the views
    address rows by key, and fixed keys keep the v1 legs
    deterministic).
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM users WHERE email = 'replay-admin@example.com'")
        admin_id = str(cur.fetchone()["id"])
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO file_assets
               (id, attributes, asset, created_by_id, workspace_id,
                user_id, project_id, entity_type, entity_identifier,
                is_deleted, deleted_at, is_archived, is_uploaded, size,
                storage_metadata, created_at, updated_at)
            VALUES (%s, '{"name": "back.txt", "type": "image/png", "size": 100}',
                    %s, %s, %s, NULL, NULL, 'ISSUE_ATTACHMENT', NULL,
                    false, NULL, false, true, 100, '{}', now(), now())
            """,
            (WS_ROW_ID, f"{ws_id}/{WS_ROW_LEAF}", admin_id, ws_id),
        )
        cur.execute(
            """
            INSERT INTO file_assets
               (id, attributes, asset, created_by_id, workspace_id,
                user_id, project_id, entity_type, entity_identifier,
                is_deleted, deleted_at, is_archived, is_uploaded, size,
                storage_metadata, created_at, updated_at)
            VALUES (%s, '{"name": "mine.txt", "type": "image/png", "size": 100}',
                    %s, %s, NULL, %s, NULL, 'USER_AVATAR', NULL,
                    false, NULL, false, true, 100, '{}', now(), now())
            """,
            (USER_ROW_ID, USER_ROW_KEY, admin_id, admin_id),
        )
    conn.commit()


SESSIONS = (
    Session(
        name="app_assets_lifecycle",
        domain="app_assets",
        source="app_assets/test_user_assets_v2.py, test_workspace_assets.py, test_project_assets.py, test_duplicate.py, test_static_restore_check.py",
        auth_users=("admin", "member", "guest", "outsider"),
        requests=(
            # User assets v2: mint -> invalid-entity 400 -> invalid
            # file 400 -> confirm (avatar link follows) -> member
            # 404s -> delete (link cleared).
            Request(
                "POST", _U + "/",
                route="/api/assets/v2/user-assets/",
                json={"name": "avatar.png", "type": "image/png", "size": 512,
                      "entity_type": "USER_AVATAR"},
                auth="admin",
                capture={"ua": "asset_id", "ukey": "upload_data/fields/key"},
                body_blank_paths=list(_PRESIGN_BLANKS),
                expect_status=200,
            ),
            Request(
                "POST", _U + "/",
                route="/api/assets/v2/user-assets/",
                json={"name": "avatar.png", "type": "image/png", "size": 512,
                      "entity_type": "WORKSPACE_LOGO"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _U + "/",
                route="/api/assets/v2/user-assets/",
                json={"name": "doc.pdf", "type": "application/pdf", "size": 512,
                      "entity_type": "USER_AVATAR"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _U + "/{ua}/",
                route="/api/assets/v2/user-assets/{pk}/",
                json={"attributes": {"name": "avatar.png", "picked": True}},
                auth="admin", expect_status=204,
            ),
            Request(
                "PATCH", _U + "/{ua}/",
                route="/api/assets/v2/user-assets/{pk}/",
                json={},
                auth="member", expect_status=404,
            ),
            Request(
                "DELETE", _U + "/{ua}/",
                route="/api/assets/v2/user-assets/{pk}/",
                auth="member", expect_status=404,
            ),
            Request(
                "DELETE", _U + "/{ua}/",
                route="/api/assets/v2/user-assets/{pk}/",
                auth="admin", expect_status=204,
            ),
            # Workspace assets v2: mint -> defaults -> clamped size ->
            # invalid 400s -> pre-upload 404 -> confirm (logo link
            # follows) -> unknown/wrong-slug 404s.
            Request(
                "POST", _W + "/",
                route="/api/assets/v2/workspaces/{slug}/",
                json={"name": "logo.png", "type": "image/png", "size": 1024,
                      "entity_type": "WORKSPACE_LOGO",
                      "entity_identifier": "{ws_id}"},
                auth="admin",
                capture={"wa": "asset_id", "wkey": "upload_data/fields/key"},
                body_blank_paths=list(_PRESIGN_BLANKS),
                expect_status=200,
            ),
            Request(
                "POST", _W + "/",
                route="/api/assets/v2/workspaces/{slug}/",
                json={"name": "d.png", "entity_type": "WORKSPACE_LOGO",
                      "entity_identifier": "{ws_id}"},
                auth="admin",
                capture={"wa2": "asset_id", "wkey2": "upload_data/fields/key"},
                body_blank_paths=list(_PRESIGN_BLANKS),
                expect_status=200,
            ),
            Request(
                "POST", _W + "/",
                route="/api/assets/v2/workspaces/{slug}/",
                json={"name": "big.png", "type": "image/png", "size": 10**9,
                      "entity_type": "WORKSPACE_LOGO",
                      "entity_identifier": "{ws_id}"},
                auth="admin",
                capture={"wa3": "asset_id", "wkey3": "upload_data/fields/key"},
                body_blank_paths=list(_PRESIGN_BLANKS),
                expect_status=200,
            ),
            Request(
                "POST", _W + "/",
                route="/api/assets/v2/workspaces/{slug}/",
                json={"name": "x.png", "type": "image/png", "size": 10,
                      "entity_type": "NOPE", "entity_identifier": "{ws_id}"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _W + "/",
                route="/api/assets/v2/workspaces/{slug}/",
                json={"name": "doc.pdf", "type": "application/pdf", "size": 10,
                      "entity_type": "WORKSPACE_LOGO",
                      "entity_identifier": "{ws_id}"},
                auth="admin", expect_status=400,
            ),
            Request(
                "GET", _W + "/{wa3}/",
                route="/api/assets/v2/workspaces/{slug}/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "PATCH", _W + "/{wa}/",
                route="/api/assets/v2/workspaces/{slug}/{pk}/",
                json={"attributes": {"name": "logo.png", "custom": 7}},
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", _W + "/" + MISSING_ID + "/",
                route="/api/assets/v2/workspaces/{slug}/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "GET", "/api/assets/v2/workspaces/no-such-ws/{wa}/",
                route="/api/assets/v2/workspaces/{slug}/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "PATCH", _W + "/" + MISSING_ID + "/",
                route="/api/assets/v2/workspaces/{slug}/{pk}/",
                json={},
                auth="admin", expect_status=404,
            ),
            # Workspace delete -> gone from detail + check -> restore
            # round trip -> idempotent restore -> unknown 404.
            Request(
                "DELETE", _W + "/{wa2}/",
                route="/api/assets/v2/workspaces/{slug}/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", _W + "/{wa2}/",
                route="/api/assets/v2/workspaces/{slug}/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "GET", _W + "/check/{wa2}/",
                route="/api/assets/v2/workspaces/{slug}/check/{pk}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _W + "/restore/{wa2}/",
                route="/api/assets/v2/workspaces/{slug}/restore/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", _W + "/check/{wa2}/",
                route="/api/assets/v2/workspaces/{slug}/check/{pk}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _W + "/restore/{wa}/",
                route="/api/assets/v2/workspaces/{slug}/restore/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", _W + "/restore/" + MISSING_ID + "/",
                route="/api/assets/v2/workspaces/{slug}/restore/{pk}/",
                auth="admin", expect_status=404,
            ),
            # Project assets v2: mint -> removed PROJECT_COVER 400
            # (PDASHOSS01-237) -> invalid 400 -> guest mint / outsider 403 ->
            # round trip (pre-upload 404, confirm, delete).
            Request(
                "POST", _P + "/",
                route="/api/assets/v2/workspaces/{slug}/projects/{project_id}/",
                json={"name": "desc.png", "type": "image/png", "size": 200,
                      "entity_type": "ISSUE_DESCRIPTION",
                      "entity_identifier": None},
                auth="admin",
                capture={"pa": "asset_id", "pkey": "upload_data/fields/key"},
                body_blank_paths=list(_PRESIGN_BLANKS),
                expect_status=200,
            ),
            Request(
                "POST", _P + "/",
                route="/api/assets/v2/workspaces/{slug}/projects/{project_id}/",
                json={"name": "cover.png", "type": "image/png", "size": 200,
                      "entity_type": "PROJECT_COVER",
                      "entity_identifier": "{project}"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _P + "/",
                route="/api/assets/v2/workspaces/{slug}/projects/{project_id}/",
                json={"name": "x.png", "type": "image/png", "size": 200,
                      "entity_type": "NOPE", "entity_identifier": None},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _P + "/",
                route="/api/assets/v2/workspaces/{slug}/projects/{project_id}/",
                json={"name": "g.png", "type": "image/png", "size": 200,
                      "entity_type": "ISSUE_DESCRIPTION",
                      "entity_identifier": None},
                auth="guest",
                capture={"pa_g": "asset_id", "pkey_g": "upload_data/fields/key"},
                body_blank_paths=list(_PRESIGN_BLANKS),
                expect_status=200,
            ),
            Request(
                "POST", _P + "/",
                route="/api/assets/v2/workspaces/{slug}/projects/{project_id}/",
                json={"name": "o.png", "type": "image/png", "size": 200,
                      "entity_type": "ISSUE_DESCRIPTION",
                      "entity_identifier": None},
                auth="outsider", expect_status=403,
            ),
            Request(
                "GET", _P + "/{pa}/",
                route="/api/assets/v2/workspaces/{slug}/projects/{project_id}/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "PATCH", _P + "/{pa}/",
                route="/api/assets/v2/workspaces/{slug}/projects/{project_id}/{pk}/",
                json={},
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _P + "/{pa}/",
                route="/api/assets/v2/workspaces/{slug}/projects/{project_id}/{pk}/",
                auth="admin", expect_status=204,
            ),
            # Bulk attach: one WORKSPACE_LOGO row (no bulk arm — the
            # PROJECT_COVER arm left with PDASHOSS01-237 — so 204 with no
            # writes) -> empty 400 -> unknown 404 -> outsider 403.
            Request(
                "POST", _W + "/",
                route="/api/assets/v2/workspaces/{slug}/",
                json={"name": "c1.png", "type": "image/png", "size": 60,
                      "entity_type": "WORKSPACE_LOGO",
                      "entity_identifier": "{ws_id}"},
                auth="admin",
                capture={"bc": "asset_id", "bkey": "upload_data/fields/key"},
                body_blank_paths=list(_PRESIGN_BLANKS),
                expect_status=200,
            ),
            Request(
                "POST", _P + "/{bc}/bulk/",
                route="/api/assets/v2/workspaces/{slug}/projects/{project_id}/{entity_id}/bulk/",
                json={"asset_ids": ["{bc}"]},
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", _P + "/" + MISSING_ID + "/bulk/",
                route="/api/assets/v2/workspaces/{slug}/projects/{project_id}/{entity_id}/bulk/",
                json={"asset_ids": []},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _P + "/" + MISSING_ID + "/bulk/",
                route="/api/assets/v2/workspaces/{slug}/projects/{project_id}/{entity_id}/bulk/",
                json={"asset_ids": [MISSING_ID]},
                auth="admin", expect_status=404,
            ),
            Request(
                "POST", _P + "/{bc}/bulk/",
                route="/api/assets/v2/workspaces/{slug}/projects/{project_id}/{entity_id}/bulk/",
                json={"asset_ids": ["{bc}"]},
                auth="outsider", expect_status=403,
            ),
            # Duplicate error legs (the success leg is storage-
            # outcome-dependent — see module docstring): invalid
            # entity 400 -> unknown asset 404 -> outsider 403.
            Request(
                "POST", _W + "/duplicate-assets/{wa}/",
                route="/api/assets/v2/workspaces/{slug}/duplicate-assets/{pk}/",
                json={"entity_type": "NOPE", "entity_id": "{ws_id}"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _W + "/duplicate-assets/" + MISSING_ID + "/",
                route="/api/assets/v2/workspaces/{slug}/duplicate-assets/{pk}/",
                json={"entity_type": "WORKSPACE_LOGO", "entity_id": "{ws_id}"},
                auth="admin", expect_status=404,
            ),
            Request(
                "POST", _W + "/duplicate-assets/{wa}/",
                route="/api/assets/v2/workspaces/{slug}/duplicate-assets/{pk}/",
                json={"entity_type": "WORKSPACE_LOGO", "entity_id": "{ws_id}"},
                auth="outsider", expect_status=403,
            ),
        ),
    ),
    Session(
        name="app_assets_legacy",
        domain="app_assets",
        source="app_assets/test_legacy_v1.py",
        auth_users=("admin", "member"),
        seed=_seed_legacy,
        static_context={
            "wsrow": WS_ROW_ID,
            "wsleaf": WS_ROW_LEAF,
            "urow": USER_ROW_ID,
            "ukey": USER_ROW_KEY,
        },
        requests=(
            # Workspace v1 rows: read -> missing-key 200 -> real
            # upload 500 (ported storage bug) -> no-file 400 ->
            # unknown-workspace 404.
            Request(
                "GET", "/api/workspaces/file-assets/{ws_id}/{wsleaf}/",
                route="/api/workspaces/file-assets/{workspace_id}/{key}/",
                auth="admin", expect_status=200,
                # PIDASHCONV-930 landed (Rust renders the DRF
                # absolute asset URL like Django); the host is a
                # topology port, uncomparable exactly, so the leaf
                # still blanks.
                body_blank_paths=["data/0/asset"],
            ),
            Request(
                "GET", "/api/workspaces/file-assets/{ws_id}/no-such-file.txt/",
                route="/api/workspaces/file-assets/{workspace_id}/{key}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", "/api/workspaces/{ws}/file-assets/",
                route="/api/workspaces/{slug}/file-assets/",
                files={"asset": ("f.txt", b"hello", "text/plain")},
                auth="admin", expect_status=500,
            ),
            Request(
                "POST", "/api/workspaces/{ws}/file-assets/",
                route="/api/workspaces/{slug}/file-assets/",
                form={},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", "/api/workspaces/no-such-ws/file-assets/",
                route="/api/workspaces/{slug}/file-assets/",
                files={"asset": ("f.txt", b"hello", "text/plain")},
                auth="admin", expect_status=404,
            ),
            # Workspace v1 delete (sets only is_deleted — the row
            # still reads back) -> unknown-key 404 -> restore round
            # trip -> unknown-key restore 404.
            Request(
                "DELETE", "/api/workspaces/file-assets/{ws_id}/{wsleaf}/",
                route="/api/workspaces/file-assets/{workspace_id}/{key}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", "/api/workspaces/file-assets/{ws_id}/{wsleaf}/",
                route="/api/workspaces/file-assets/{workspace_id}/{key}/",
                auth="admin", expect_status=200,
                # PIDASHCONV-930 landed (Rust renders the DRF
                # absolute asset URL like Django); the host is a
                # topology port, uncomparable exactly, so the leaf
                # still blanks.
                body_blank_paths=["data/0/asset"],
            ),
            Request(
                "DELETE", "/api/workspaces/file-assets/{ws_id}/no-such-file.txt/",
                route="/api/workspaces/file-assets/{workspace_id}/{key}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "POST", "/api/workspaces/file-assets/{ws_id}/{wsleaf}/restore/",
                route="/api/workspaces/file-assets/{workspace_id}/{key}/restore/",
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", "/api/workspaces/file-assets/{ws_id}/{wsleaf}/",
                route="/api/workspaces/file-assets/{workspace_id}/{key}/",
                auth="admin", expect_status=200,
                # PIDASHCONV-930 landed (Rust renders the DRF
                # absolute asset URL like Django); the host is a
                # topology port, uncomparable exactly, so the leaf
                # still blanks.
                body_blank_paths=["data/0/asset"],
            ),
            Request(
                "POST", "/api/workspaces/file-assets/{ws_id}/no-such-file.txt/restore/",
                route="/api/workspaces/file-assets/{workspace_id}/{key}/restore/",
                auth="admin", expect_status=404,
            ),
            # User v1 rows: existing-key 500 (ported serializer bug)
            # -> missing-key 200 -> real upload 500 -> no-file 400 ->
            # JSON 415 -> member 404 -> delete 204.
            Request(
                "GET", "/api/users/file-assets/{ukey}/",
                route="/api/users/file-assets/{key}/",
                auth="admin", expect_status=500,
            ),
            Request(
                "GET", "/api/users/file-assets/no-such-key/",
                route="/api/users/file-assets/{key}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", "/api/users/file-assets/",
                route="/api/users/file-assets/",
                files={"asset": ("u.txt", b"hi", "text/plain")},
                auth="admin", expect_status=500,
            ),
            Request(
                "POST", "/api/users/file-assets/",
                route="/api/users/file-assets/",
                form={},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", "/api/users/file-assets/",
                route="/api/users/file-assets/",
                json={"x": 1},
                auth="admin", expect_status=415,
            ),
            Request(
                "DELETE", "/api/users/file-assets/{ukey}/",
                route="/api/users/file-assets/{key}/",
                auth="member", expect_status=404,
            ),
            Request(
                "DELETE", "/api/users/file-assets/{ukey}/",
                route="/api/users/file-assets/{key}/",
                auth="admin", expect_status=204,
            ),
        ),
    ),
)
