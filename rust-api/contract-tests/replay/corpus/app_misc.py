# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app-tier miscellaneous write flows (PIDASHCONV-823).

Built from the ``app_workspace`` (test_api_tokens.py, test_user.py),
``app_analytics`` (test_exporter.py, test_analytic_shapes.py) and
``app_integrations`` (test_external.py) contract suites: API tokens,
own-user update/deactivate, email flows, profile/onboard/tour,
connected accounts, analytic views, issue/analytics exports and the
two AI assistants. Session-cookie auth (admin/member/outsider/
anonymous), like the suites.

Deliberately narrowed legs: the email success paths need the random
verification code (mailed + stored, knowable nowhere — the 400 legs
cover both pairs); the export-issues token is backend-minted
(``generated_columns`` + ``payload_db_values``, the labels.color
pattern). The PATCH-only profile/onboard/tour views are covered by
their PATCH legs (direct probes showed POST is 405-JSON on both
backends, but POST is not an inventory pair there so no leg claims
it); collection-DELETE accounts is the same ported 500 envelope on
both (an inventory pair, covered).
"""

from psycopg.rows import dict_row

from ..session import Request, Session

# Fixed ids seeded by _seed_misc (static_context).
MTOK_ID = "c7c7c7c7-0000-4000-8000-000000000001"
PROF_ID = "c7c7c7c7-0000-4000-8000-000000000002"
PROF_MEMBER_ID = "c7c7c7c7-0000-4000-8000-000000000007"
ACCT_ADMIN_ID = "c7c7c7c7-0000-4000-8000-000000000003"
ACCT_MEMBER_ID = "c7c7c7c7-0000-4000-8000-000000000004"
INST_ID = "c7c7c7c7-0000-4000-8000-000000000005"
INST_ADMIN_ID = "c7c7c7c7-0000-4000-8000-000000000006"

MISSING_ID = "00000000-0000-0000-0000-000000000000"

_T = "/api/users/api-tokens"
_M = "/api/users/me"
_AC = "/api/users/me/accounts"
_AV = "/api/workspaces/{ws}/analytic-view"


def _seed_misc(conn) -> None:
    """Profile for admin, one token for member, one connected account
    each for admin/member, and an instance (+ outsider grant, for the
    deactivate-blocked leg).

    The INSERTs mirror ``_harness.db`` (make_api_token, make_account,
    make_instance, make_admin) and the app_workspace profile seed (same
    columns); the member token value is fixed (the world never renders
    raw tokens outside the create response).
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM users WHERE email = 'replay-admin@example.com'")
        admin_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM users WHERE email = 'replay-member@example.com'")
        member_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM users WHERE email = 'replay-outsider@example.com'")
        outsider_id = str(cur.fetchone()["id"])
    with conn.cursor() as cur:
        for prof_id, user_id in ((PROF_ID, admin_id), (PROF_MEMBER_ID, member_id)):
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
                VALUES (now(), now(), %s, '{}', false,
                    '{"profile_complete": false}', false, 'INDIA', false, '',
                    %s, false, '{}', false, 'en', false, 0, true, '#ffffff', '{}',
                    false, false, false, 'full', '{}', '{}')
                """,
                (prof_id, user_id),
            )
        cur.execute(
            """
            INSERT INTO api_tokens (id, token, label, description, is_active,
                user_type, user_id, workspace_id, is_service,
                allowed_rate_limit, created_at, updated_at)
            VALUES (%s, 'pi_dash_api_c7memberfixedtoken0001', 'Member token',
                    '', true, 0, %s, NULL, false, '', now(), now())
            """,
            (MTOK_ID, member_id),
        )
        for aid, uid, paid in (
            (ACCT_ADMIN_ID, admin_id, "c7-admin-acc"),
            (ACCT_MEMBER_ID, member_id, "c7-member-acc"),
        ):
            cur.execute(
                """
                INSERT INTO accounts
                   (created_at, updated_at, id, user_id, provider_account_id,
                    provider, access_token, id_token, metadata, last_connected_at)
                VALUES (now(), now(), %s, %s, %s, 'google',
                        'access-token', '', '{}', now())
                """,
                (aid, uid, paid),
            )
        cur.execute(
            """
            INSERT INTO instances (created_at, updated_at, id, instance_name,
                instance_id, current_version, last_checked_at,
                is_telemetry_enabled, is_support_required, is_setup_done,
                is_signup_screen_visited, is_verified, domain, edition,
                is_test, is_current_version_deprecated)
            VALUES (now(), now(), %s, 'Contract Instance', 'instance-c7c7c7c7',
                    '1.0.0', now(), true, true, true, false, false, '',
                    'PI_DASH_COMMUNITY', false, false)
            """,
            (INST_ID,),
        )
        cur.execute(
            """
            INSERT INTO instance_admins
               (created_at, updated_at, id, role, is_verified, instance_id, user_id)
            VALUES (now(), now(), %s, 20, true, %s, %s)
            """,
            (INST_ADMIN_ID, INST_ID, outsider_id),
        )
    conn.commit()


SESSIONS = (
    Session(
        name="app_misc_lifecycle",
        domain="app_misc",
        source="app_workspace/test_api_tokens.py, test_user.py, app_analytics/test_exporter.py, test_analytic_shapes.py, app_integrations/test_external.py",
        auth_users=("admin", "member", "guest", "outsider"),
        seed=_seed_misc,
        static_context={
            "mtok": MTOK_ID,
            "acct": ACCT_ADMIN_ID,
            "acct_member": ACCT_MEMBER_ID,
        },
        # The export-issues token is uuid4-minted per backend
        # (models/exporter.py:generate_token): blank the stored cell
        # and align the queued token_id by live existence, the
        # labels.color pattern.
        generated_columns={
            "exporters": ["token"],
            # Deactivate sets a fresh random password per backend
            # (user/base.py: set_password(uuid4().hex)); the hash is
            # doubly unknowable (random value + random salt). No
            # other leg in this session writes passwords.
            "users": ["password"],
        },
        payload_db_values={
            "pi_dash.bgtasks.export_task.issue_export_task": [
                ("kwargs/token_id", "exporters", "token"),
            ],
        },
        requests=(
            # API tokens: create -> default-label mint -> expiry mint
            # -> list (own only) -> detail -> unknown 404 -> other-user
            # 404 -> patch full -> patch partial -> patch 404 ->
            # delete other-user 404 (kept) -> delete unknown 404 -> delete
            # -> gone.
            Request(
                "POST", _T + "/",
                route="/api/users/api-tokens/",
                json={"label": "Deploy", "description": "CI deploys"},
                auth="admin",
                capture={"tok": "id", "raw": "token"},
                expect_status=201,
            ),
            Request(
                "POST", _T + "/",
                route="/api/users/api-tokens/",
                json={},
                auth="admin",
                capture={"tok2": "id", "raw2": "token", "label2": "label"},
                expect_status=201,
            ),
            Request(
                "POST", _T + "/",
                route="/api/users/api-tokens/",
                json={"label": "Expiring", "expired_at": "2030-01-01T00:00:00Z"},
                auth="admin",
                capture={"tok3": "id", "raw3": "token"},
                expect_status=201,
            ),
            Request(
                "GET", _T + "/",
                route="/api/users/api-tokens/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _T + "/{tok}/",
                route="/api/users/api-tokens/{pk}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _T + "/" + MISSING_ID + "/",
                route="/api/users/api-tokens/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "GET", _T + "/{mtok}/",
                route="/api/users/api-tokens/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "PATCH", _T + "/{tok}/",
                route="/api/users/api-tokens/{pk}/",
                json={"label": "New", "description": "Updated"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _T + "/{tok}/",
                route="/api/users/api-tokens/{pk}/",
                json={"label": "Only"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _T + "/" + MISSING_ID + "/",
                route="/api/users/api-tokens/{pk}/",
                json={"label": "New"},
                auth="admin", expect_status=404,
            ),
            Request(
                "DELETE", _T + "/{mtok}/",
                route="/api/users/api-tokens/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "DELETE", _T + "/" + MISSING_ID + "/",
                route="/api/users/api-tokens/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "DELETE", _T + "/{tok2}/",
                route="/api/users/api-tokens/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", _T + "/{tok2}/",
                route="/api/users/api-tokens/{pk}/",
                auth="admin", expect_status=404,
            ),
            # Own user: read -> update names -> URL name 400 ->
            # email read-only -> anon 401.
            Request(
                "GET", _M + "/",
                route="/api/users/me/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _M + "/",
                route="/api/users/me/",
                json={"first_name": "Ada", "last_name": "Lovelace"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _M + "/",
                route="/api/users/me/",
                json={"first_name": "https://evil.example/x"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _M + "/",
                route="/api/users/me/",
                json={"email": "hacked@example.com"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _M + "/",
                route="/api/users/me/",
                json={"first_name": "Anon"},
                auth="anonymous", expect_status=401,
            ),
            # Email flows (error legs only — the success path needs
            # the mailed random code, knowable nowhere): bad format
            # -> current -> taken -> requires-code -> stale code.
            Request(
                "POST", _M + "/email/generate-code/",
                route="/api/users/me/email/generate-code/",
                json={"email": "not-an-email"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _M + "/email/generate-code/",
                route="/api/users/me/email/generate-code/",
                json={"email": "replay-admin@example.com"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _M + "/email/generate-code/",
                route="/api/users/me/email/generate-code/",
                json={"email": "replay-member@example.com"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _M + "/email/",
                route="/api/users/me/email/",
                json={"email": "newaddr@example.com"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _M + "/email/",
                route="/api/users/me/email/",
                json={"email": "newaddr@example.com", "code": "000000"},
                auth="admin", expect_status=400,
            ),
            # Profile/onboard/tour: read -> patch theme -> bad
            # namespace 400 -> onboard -> tour.
            Request(
                "GET", _M + "/profile/",
                route="/api/users/me/profile/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _M + "/profile/",
                route="/api/users/me/profile/",
                json={"theme": {"mode": "dark"}},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _M + "/profile/",
                route="/api/users/me/profile/",
                json={"settings": {"nope": {"k": 1}}},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _M + "/onboard/",
                route="/api/users/me/onboard/",
                json={"is_onboarded": True},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _M + "/tour-completed/",
                route="/api/users/me/tour-completed/",
                json={"is_tour_completed": True},
                auth="admin", expect_status=200,
            ),
            # Connected accounts: list (own only) -> detail ->
            # unknown 404 -> other-user 404 -> delete other-user 404 -> delete
            # -> gone -> collection DELETE 500 (ported TypeError;
            # method parity, not an inventory pair).
            Request(
                "GET", _AC + "/",
                route="/api/users/me/accounts/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _AC + "/{acct}/",
                route="/api/users/me/accounts/{pk}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _AC + "/" + MISSING_ID + "/",
                route="/api/users/me/accounts/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "GET", _AC + "/{acct_member}/",
                route="/api/users/me/accounts/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "DELETE", _AC + "/{acct_member}/",
                route="/api/users/me/accounts/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "DELETE", _AC + "/{acct}/",
                route="/api/users/me/accounts/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", _AC + "/{acct}/",
                route="/api/users/me/accounts/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "DELETE", _AC + "/",
                route="/api/users/me/accounts/",
                auth="admin", expect_status=500,
            ),
            # Analytic views: list -> create -> detail -> rename ->
            # issues/ 404 (no such route on either backend; Rust
            # proxies Django's DEBUG page, whose host is per-backend,
            # so status-only with a marker pin) -> delete -> gone.
            Request(
                "GET", _AV + "/",
                route="/api/workspaces/{slug}/analytic-view/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _AV + "/",
                route="/api/workspaces/{slug}/analytic-view/",
                json={"name": "AV-RP", "description": "roundtrip",
                      "query_dict": {"priority": "high"}},
                auth="admin",
                capture={"avid": "id"},
                expect_status=201,
            ),
            Request(
                "GET", _AV + "/{avid}/",
                route="/api/workspaces/{slug}/analytic-view/{pk}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _AV + "/{avid}/",
                route="/api/workspaces/{slug}/analytic-view/{pk}/",
                json={"name": "AV-RP2"},
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _AV + "/{avid}/",
                route="/api/workspaces/{slug}/analytic-view/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", _AV + "/{avid}/",
                route="/api/workspaces/{slug}/analytic-view/{pk}/",
                auth="admin", expect_status=404,
            ),
            # Exports: issues POST (minted token aligned by live
            # existence) -> history shows it -> bad provider 400 ->
            # analytics POST -> bad axis 400.
            Request(
                "POST", "/api/workspaces/{ws}/export-issues/",
                route="/api/workspaces/{slug}/export-issues/",
                json={"provider": "csv", "project": ["{project}"]},
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", "/api/workspaces/{ws}/export-issues/",
                route="/api/workspaces/{slug}/export-issues/",
                params={"per_page": "10", "cursor": "10:0:0"},
                auth="admin",
                capture={"expid": "results/0/id", "exptok": "results/0/token"},
                expect_status=200,
            ),
            Request(
                "POST", "/api/workspaces/{ws}/export-issues/",
                route="/api/workspaces/{slug}/export-issues/",
                json={"provider": "xml"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", "/api/workspaces/{ws}/export-analytics/",
                route="/api/workspaces/{slug}/export-analytics/",
                json={"x_axis": "priority", "y_axis": "issue_count"},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", "/api/workspaces/{ws}/export-analytics/",
                route="/api/workspaces/{slug}/export-analytics/",
                json={"x_axis": "nope", "y_axis": "issue_count"},
                auth="admin", expect_status=400,
            ),
            # AI assistants (dummy provider key: validation branch +
            # error envelope on both sides): ws 400 -> ws 500 ->
            # outsider 403 -> project 400 -> project 500 -> guest 403.
            Request(
                "POST", "/api/workspaces/{ws}/ai-assistant/",
                route="/api/workspaces/{slug}/ai-assistant/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", "/api/workspaces/{ws}/ai-assistant/",
                route="/api/workspaces/{slug}/ai-assistant/",
                json={"task": "summarize", "prompt": "hello"},
                auth="admin", expect_status=500,
            ),
            Request(
                "POST", "/api/workspaces/{ws}/ai-assistant/",
                route="/api/workspaces/{slug}/ai-assistant/",
                json={"task": "x"},
                auth="outsider", expect_status=403,
            ),
            Request(
                "POST", "/api/workspaces/{ws}/projects/{project}/ai-assistant/",
                route="/api/workspaces/{slug}/projects/{project_id}/ai-assistant/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", "/api/workspaces/{ws}/projects/{project}/ai-assistant/",
                route="/api/workspaces/{slug}/projects/{project_id}/ai-assistant/",
                json={"task": "summarize", "prompt": "hello"},
                auth="admin", expect_status=500,
            ),
            Request(
                "POST", "/api/workspaces/{ws}/projects/{project}/ai-assistant/",
                route="/api/workspaces/{slug}/projects/{project_id}/ai-assistant/",
                json={"task": "x"},
                auth="guest", expect_status=403,
            ),
            # Deactivate, last: instance-admin outsider is blocked
            # 400 -> member deactivates 204 -> member reads 401
            # (sessions nuked) -> admin still reads 200.
            Request(
                "DELETE", _M + "/",
                route="/api/users/me/",
                auth="outsider", expect_status=400,
            ),
            Request(
                "DELETE", _M + "/",
                route="/api/users/me/",
                auth="member", expect_status=204,
            ),
            Request(
                "GET", _M + "/",
                route="/api/users/me/",
                auth="member", expect_status=401,
            ),
            Request(
                "GET", _M + "/",
                route="/api/users/me/",
                auth="admin", expect_status=200,
            ),
        ),
    ),
)
