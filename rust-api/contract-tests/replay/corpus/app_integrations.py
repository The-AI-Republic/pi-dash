# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app-tier integration write flows (PIDASHCONV-823).

Built from the ``app_integrations`` contract suite:
test_integration_git.py (provider accounts, repository binding),
test_integration_github.py (PAT connect/disconnect),
test_project_github.py (project bind/toggle/unbind) and
test_github_app.py (App install/refresh/webhook). Session-cookie auth
(admin/member/guest/outsider/anonymous), like the suite.

Upstream-dependent behaviors stay out, with the routes still covered
through offline legs: account-create success and repository-bind
success call the provider (400/403 legs cover the pairs), and the
refresh-502 leg's error TEXT is client-defined per backend (the suite
pins it loosely for the same reason). The connect-bogus-token 401 IS
in — api.github.com deterministically rejects it upstream on both
sides (probed reachable from this runner). The GitHub App config
(including the webhook HMAC secret) is seeded into
``instance_configurations``, which both backends read through the same
store, so webhook deliveries sign deterministically with a
corpus-known secret.

Order matters: each project-scoped DELETE wipes the other table's
rows (repository DELETE hard-deletes syncs; github DELETE enqueues
wipes for the sync and the binding), so the RPJ bound legs run before
either DELETE, the RPJ resumed legs run unbound, and a second binding
seeded on a second replay-ws project (RPJ2) proves the repository
toggle-on and bound DELETE. The PAT disconnect runs before the
bind-not-connected 409 it arms.
"""

import hashlib
import hmac
import json

from psycopg.rows import dict_row

from _harness import db as db_helpers
from _harness.db import ADMIN

from ..session import Request, Session

# Fixed ids seeded by _seed_integrations (static_context).
INT_ID = "b6b6b6b6-0000-4000-8000-000000000001"
WI_ID = "b6b6b6b6-0000-4000-8000-000000000002"
ACCT_ID = "b6b6b6b6-0000-4000-8000-000000000003"
REPO_ID = "b6b6b6b6-0000-4000-8000-000000000004"
SYNC_ID = "b6b6b6b6-0000-4000-8000-000000000005"
GITREPO_ID = "b6b6b6b6-0000-4000-8000-000000000006"
BIND_ID = "b6b6b6b6-0000-4000-8000-000000000007"
BIND2_ID = "b6b6b6b6-0000-4000-8000-000000000008"
PROJ2_ID = "b6b6b6b6-0000-4000-8000-000000000009"

MISSING_ID = "00000000-0000-0000-0000-000000000000"

# Corpus-known webhook HMAC secret, seeded into instance_configurations
# (both backends read it through the same config store).
_WEBHOOK_SECRET = "replay-webhook-secret-b6"

# Fixed delivery ids for the webhook legs.
_D1 = "b6b6b6b6-1111-4111-8111-111111111111"
_D2 = "b6b6b6b6-2222-4222-8222-222222222222"
_D3 = "b6b6b6b6-3333-4333-8333-333333333333"
_D4 = "b6b6b6b6-4444-4444-8444-444444444444"
_D5 = "b6b6b6b6-5555-4555-8555-555555555555"

_A = "/api/workspaces/{ws}/integrations/git/accounts"
_R = "/api/workspaces/{ws}/projects/{project}/repository"
_RX = "/api/workspaces/{ws}/projects/{project2}/repository"
_G = "/api/workspaces/{ws}/integrations/github"
_P = "/api/workspaces/{ws}/projects/{project}/github"
_I = "/api/users/me/integrations/github/app"
_WH = "/api/integrations/github/app/webhook"


def _sign(body: bytes) -> str:
    return "sha256=" + hmac.new(_WEBHOOK_SECRET.encode(), body, hashlib.sha256).hexdigest()


def _delivery(payload: dict | bytes, event: str, delivery_id: str, signature: str | None = None):
    """Raw webhook body + headers, mirroring the suite's webhook_post."""
    body = payload if isinstance(payload, bytes) else json.dumps(payload).encode()
    return body, {
        "Content-Type": "application/json",
        "X-Hub-Signature-256": signature if signature is not None else _sign(body),
        "X-GitHub-Event": event,
        "X-GitHub-Delivery": delivery_id,
    }


def _seed_integrations(conn) -> None:
    """GitHub App config + PAT integration + git account + repo sync +
    git binding, all fixed ids.

    The INSERTs mirror ``_harness.seed.Seeder`` (ensure_github_app_config,
    ensure_github_integration, create_workspace_integration with
    connected=True, create_git_provider_account, create_github_repo_sync);
    the git repository + binding mirror the ``git_repositories`` /
    ``git_repository_bindings`` columns. The API token for the workspace
    integration reuses ``db_helpers.create_api_token`` (random id, cloned
    to the Rust copy with everything else).
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM projects WHERE identifier = 'RPJ'")
        project_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM users WHERE email = 'replay-admin@example.com'")
        admin_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM projects WHERE identifier = 'RPJ2'")
        rpj2 = cur.fetchone()
        if rpj2 is None:
            # Second project under replay-ws for the repository
            # bound-toggle/bound-DELETE legs (RPX lives in the other
            # workspace, so it cannot host them). Inlined from
            # db_helpers.create_project (same columns/defaults) with
            # the fixed id the paths address.
            with conn.cursor() as recur:
                recur.execute(
                    """
                    INSERT INTO projects (id, name, description, network, identifier,
                        workspace_id, created_by_id, cycle_view, module_view,
                        issue_views_view, page_view, intake_view, archive_in, close_in,
                        logo_props, is_time_tracking_enabled, is_issue_type_enabled,
                        guest_view_all_features, timezone, members_can_edit_states,
                        repo_url, base_branch, agent_default_interval_seconds,
                        agent_default_max_ticks, agent_ticking_enabled, is_default,
                        agent_review_default_interval_seconds, default_agent_executor,
                        agent_test_default_interval_seconds, created_at, updated_at, default_work_type)
                    VALUES (%s, 'Replay Second Project', '', 2, 'RPJ2',
                        %s, %s, true, true,
                        true, true, false, 0, 0,
                        '{}', true, true,
                        false, 'UTC', false,
                        '', '', 0,
                        0, false, false,
                        0, '', 0, now(), now(), 'software')
                    """,
                    (PROJ2_ID, ws_id, admin_id),
                )
                recur.execute(
                    """
                    INSERT INTO project_identifiers (name, project_id, workspace_id,
                        created_by_id, created_at, updated_at)
                    VALUES ('RPJ2', %s, %s, %s, now(), now())
                    """,
                    (PROJ2_ID, ws_id, admin_id),
                )
            db_helpers.add_project_member(conn, ws_id, PROJ2_ID, admin_id, ADMIN)
    token = db_helpers.create_api_token(conn, admin_id, "github-integration-shim")
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO integrations
               (id, title, provider, network, description, author,
                webhook_url, webhook_secret, redirect_url, metadata,
                verified, created_at, updated_at)
            VALUES (%s, 'GitHub', 'github', 2,
                    '{"summary": "Mirror GitHub issues into Pi Dash projects."}',
                    '', '', '', '', '{}', true, now(), now())
            """,
            (INT_ID,),
        )
        for key, value in (
            ("GITHUB_APP_ID", "replay-91-app-id"),
            ("GITHUB_APP_SLUG", "replay-91-app-slug"),
            ("GITHUB_APP_PRIVATE_KEY", "replay-dummy-private-key"),
            ("GITHUB_APP_WEBHOOK_SECRET", _WEBHOOK_SECRET),
            ("GITHUB_APP_CLIENT_ID", "replay-91-client-id"),
            ("GITHUB_APP_CLIENT_SECRET", "replay-dummy-client-secret"),
        ):
            cur.execute(
                """
                INSERT INTO instance_configurations
                   (id, key, value, category, is_encrypted, created_at, updated_at)
                VALUES (gen_random_uuid(), %s, %s, 'GITHUB', false, now(), now())
                """,
                (key, value),
            )
        cur.execute(
            """
            INSERT INTO workspace_integrations
               (id, metadata, config, actor_id, api_token_id,
                integration_id, workspace_id, created_at, updated_at)
            VALUES (%s, '{}',
                    '{"auth_type": "pat", "token": "replay91-seeded-token",
                      "github_user_login": "contract-octocat",
                      "verified_at": "2026-01-01T00:00:00+00:00"}',
                    %s, %s, %s, %s, now(), now())
            """,
            (WI_ID, admin_id, token["id"], INT_ID, ws_id),
        )
        cur.execute(
            """
            INSERT INTO git_provider_accounts
               (id, provider, host_url, auth_type, external_account_id,
                external_account_login, display_name, capabilities,
                credential_config, status, verified_at, last_check_error,
                metadata, workspace_id, created_at, updated_at)
            VALUES (%s, 'github', 'https://github.com', 'pat', 'contract:b6b6b6b6',
                    'contract-octocat', 'contract-octocat', '{}', '{}',
                    'connected', now(), '', '{}', %s, now(), now())
            """,
            (ACCT_ID, ws_id),
        )
        cur.execute(
            """
            INSERT INTO github_repositories
               (id, name, url, config, repository_id, owner,
                project_id, workspace_id, created_at, updated_at)
            VALUES (%s, 'contract-repo-b6',
                    'https://github.com/contract-owner-b6/contract-repo-b6',
                    '{}', 42424242, 'contract-owner-b6', %s, %s, now(), now())
            """,
            (REPO_ID, project_id, ws_id),
        )
        cur.execute(
            """
            INSERT INTO github_repository_syncs
               (id, credentials, actor_id, project_id, repository_id,
                workspace_id, workspace_integration_id,
                is_sync_enabled, last_sync_error, created_at, updated_at)
            VALUES (%s, '{}', %s, %s, %s, %s, %s, false, '', now(), now())
            """,
            (SYNC_ID, admin_id, project_id, REPO_ID, ws_id, WI_ID),
        )
        cur.execute(
            """
            INSERT INTO git_repositories
               (id, provider, host_url, external_id, namespace, name,
                full_name, web_url, clone_url_http, clone_url_ssh,
                default_branch, is_private, metadata,
                created_at, updated_at)
            VALUES (%s, 'github', 'https://github.com', '42424243',
                    'contract-owner-b6', 'contract-git-b6',
                    'contract-owner-b6/contract-git-b6',
                    'https://github.com/contract-owner-b6/contract-git-b6',
                    'https://github.com/contract-owner-b6/contract-git-b6.git',
                    'git@github.com:contract-owner-b6/contract-git-b6.git',
                    'main', false, '{}', now(), now())
            """,
            (GITREPO_ID,),
        )
        cur.execute(
            """
            INSERT INTO git_repository_bindings
               (id, repository_id, provider_account_id, project_id,
                workspace_id, actor_id, is_sync_enabled, clone_auth_mode,
                last_synced_at, last_sync_error, metadata,
                created_at, updated_at)
            VALUES (%s, %s, %s, %s, %s, %s, false, 'public',
                    NULL, '', '{}', now(), now())
            """,
            (BIND_ID, GITREPO_ID, ACCT_ID, project_id, ws_id, admin_id),
        )
        cur.execute(
            """
            INSERT INTO git_repository_bindings
               (id, repository_id, provider_account_id, project_id,
                workspace_id, actor_id, is_sync_enabled, clone_auth_mode,
                last_synced_at, last_sync_error, metadata,
                created_at, updated_at)
            VALUES (%s, %s, %s, %s, %s, %s, false, 'public',
                    NULL, '', '{}', now(), now())
            """,
            (BIND2_ID, GITREPO_ID, ACCT_ID, PROJ2_ID, ws_id, admin_id),
        )
    conn.commit()


_PING_BODY, _PING_HEADERS = _delivery({"zen": "contract"}, "ping", _D1)
_BAD_JSON_BODY, _BAD_JSON_HEADERS = _delivery(b"{not json", "ping", _D2)
_PR_BODY, _PR_HEADERS = _delivery(
    {"action": "opened", "pull_request": {"number": 1}, "repository": {}},
    "pull_request", _D3,
)
_INSTALL_BODY, _INSTALL_HEADERS = _delivery(
    {"action": "created", "installation": {"id": 987654321}},
    "installation", _D4,
)
_STAR_BODY, _STAR_HEADERS = _delivery({"action": "x"}, "star", _D5)
_BADSIG_BODY = json.dumps({"zen": "x"}).encode()


SESSIONS = (
    Session(
        name="app_integrations_lifecycle",
        domain="app_integrations",
        source="app_integrations/test_integration_git.py, test_integration_github.py, test_project_github.py, test_github_app.py",
        auth_users=("admin", "member", "guest", "outsider"),
        seed=_seed_integrations,
        static_context={
            "acct": ACCT_ID,
            "sync": SYNC_ID,
            "bind": BIND_ID,
            "project2": PROJ2_ID,
        },
        # Install sessions stamp expires_at = now()+15m per backend.
        timestamp_columns={
            "github_app_install_sessions": ["expires_at"],
        },
        requests=(
            # Git provider accounts: providers -> guest 403 ->
            # accounts list -> create validations -> member 403 ->
            # detail -> unknown 404 -> cross-tenant 403 -> revoke
            # member 403 -> revoke -> status reads revoked.
            Request(
                "GET", "/api/workspaces/{ws}/integrations/git/providers/",
                route="/api/workspaces/{slug}/integrations/git/providers/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", "/api/workspaces/{ws}/integrations/git/providers/",
                route="/api/workspaces/{slug}/integrations/git/providers/",
                auth="guest", expect_status=403,
            ),
            Request(
                "GET", _A + "/",
                route="/api/workspaces/{slug}/integrations/git/accounts/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _A + "/",
                route="/api/workspaces/{slug}/integrations/git/accounts/",
                json={"provider": "bitbucket", "token": "x"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _A + "/",
                route="/api/workspaces/{slug}/integrations/git/accounts/",
                json={"provider": "github"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _A + "/",
                route="/api/workspaces/{slug}/integrations/git/accounts/",
                json={"provider": "github", "token": "x"},
                auth="member", expect_status=403,
            ),
            Request(
                "GET", _A + "/{acct}/",
                route="/api/workspaces/{slug}/integrations/git/accounts/{pk}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _A + "/" + MISSING_ID + "/",
                route="/api/workspaces/{slug}/integrations/git/accounts/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "GET", _A + "/{acct}/",
                route="/api/workspaces/{slug}/integrations/git/accounts/{pk}/",
                auth="outsider", expect_status=403,
            ),
            Request(
                "DELETE", _A + "/{acct}/",
                route="/api/workspaces/{slug}/integrations/git/accounts/{pk}/",
                auth="member", expect_status=403,
            ),
            Request(
                "DELETE", _A + "/{acct}/",
                route="/api/workspaces/{slug}/integrations/git/accounts/{pk}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _A + "/{acct}/",
                route="/api/workspaces/{slug}/integrations/git/accounts/{pk}/",
                auth="admin", expect_status=200,
            ),
            # Repository binding on RPJ (bound from seed): read ->
            # toggle validation 400 -> member 403. The resumed legs
            # run after the project-github block (see below).
            Request(
                "GET", _R + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/repository/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _R + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/repository/",
                json={"enabled": "yes"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _R + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/repository/",
                json={"enabled": True},
                auth="member", expect_status=403,
            ),
            # GitHub PAT: connected status -> member reads -> guest
            # 403 -> connect validations -> live bogus-token 401 ->
            # connect member 403 -> disconnect (clears the seeded
            # credential) -> status + repos prove it -> disconnect
            # again (idempotent) -> member 403.
            Request(
                "GET", _G + "/",
                route="/api/workspaces/{slug}/integrations/github/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _G + "/",
                route="/api/workspaces/{slug}/integrations/github/",
                auth="member", expect_status=200,
            ),
            Request(
                "GET", _G + "/",
                route="/api/workspaces/{slug}/integrations/github/",
                auth="guest", expect_status=403,
            ),
            Request(
                "POST", _G + "/connect/",
                route="/api/workspaces/{slug}/integrations/github/connect/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _G + "/connect/",
                route="/api/workspaces/{slug}/integrations/github/connect/",
                json={"token": "bogus-replay-token"},
                auth="admin", expect_status=401,
            ),
            Request(
                "POST", _G + "/connect/",
                route="/api/workspaces/{slug}/integrations/github/connect/",
                json={"token": "x"},
                auth="member", expect_status=403,
            ),
            Request(
                "POST", _G + "/disconnect/",
                route="/api/workspaces/{slug}/integrations/github/disconnect/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _G + "/",
                route="/api/workspaces/{slug}/integrations/github/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _G + "/repos/",
                route="/api/workspaces/{slug}/integrations/github/repos/",
                auth="admin", expect_status=409,
            ),
            Request(
                "POST", _G + "/disconnect/",
                route="/api/workspaces/{slug}/integrations/github/disconnect/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _G + "/disconnect/",
                route="/api/workspaces/{slug}/integrations/github/disconnect/",
                auth="member", expect_status=403,
            ),
            # Project github: bound status -> bind validations (the
            # disconnect above arms not-connected 409) -> member 403
            # -> patch bool 400 -> toggle round trip -> member 403 ->
            # DELETE bound -> gone -> DELETE unbound.
            Request(
                "GET", _P + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/github/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _P + "/bind/",
                route="/api/workspaces/{slug}/projects/{project_id}/github/bind/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _P + "/bind/",
                route="/api/workspaces/{slug}/projects/{project_id}/github/bind/",
                json={"repo_url": "https://gitlab.com/o/r"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _P + "/bind/",
                route="/api/workspaces/{slug}/projects/{project_id}/github/bind/",
                json={"repo_url": "https://github.com/o/r"},
                auth="admin", expect_status=409,
            ),
            Request(
                "POST", _P + "/bind/",
                route="/api/workspaces/{slug}/projects/{project_id}/github/bind/",
                json={"repo_url": "https://github.com/o/r"},
                auth="member", expect_status=403,
            ),
            Request(
                "PATCH", _P + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/github/",
                json={"enabled": "yes"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _P + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/github/",
                json={"enabled": True},
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _P + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/github/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _P + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/github/",
                json={"enabled": False},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _P + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/github/",
                json={"enabled": True},
                auth="member", expect_status=403,
            ),
            Request(
                "DELETE", _P + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/github/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _P + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/github/",
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _P + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/github/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _P + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/github/",
                json={"enabled": True},
                auth="admin", expect_status=404,
            ),
            # Repository on RPJ, resumed: the github DELETE wiped
            # the binding, so these run unbound -> gone -> PATCH
            # unbound 404 -> DELETE unbound -> bind validations ->
            # bind member 403.
            Request(
                "GET", _R + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/repository/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _R + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/repository/",
                json={"enabled": True},
                auth="admin", expect_status=404,
            ),
            Request(
                "DELETE", _R + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/repository/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _R + "/bind/",
                route="/api/workspaces/{slug}/projects/{project_id}/repository/bind/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _R + "/bind/",
                route="/api/workspaces/{slug}/projects/{project_id}/repository/bind/",
                json={"repo_url": "https://bitbucket.org/o/r"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _R + "/bind/",
                route="/api/workspaces/{slug}/projects/{project_id}/repository/bind/",
                json={"repo_url": "https://github.com/o/r"},
                auth="member", expect_status=403,
            ),
            # Repository on RPJ2 (second seeded binding, untouched
            # by the RPJ-scoped wipes): bound read -> toggle on ->
            # read -> DELETE bound -> gone.
            Request(
                "GET", _RX + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/repository/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _RX + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/repository/",
                json={"enabled": True},
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _RX + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/repository/",
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _RX + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/repository/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _RX + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/repository/",
                auth="admin", expect_status=200,
            ),
            # App install: missing slug 400 -> unknown ws 404 ->
            # member 403 -> create 201 (state captured; the install
            # URL's ?state= blanks by anchored regex, expires_at by
            # the timestamp keys).
            Request(
                "POST", _I + "/install/",
                route="/api/users/me/integrations/github/app/install/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _I + "/install/",
                route="/api/users/me/integrations/github/app/install/",
                json={"workspace_slug": "no-such-workspace"},
                auth="admin", expect_status=404,
            ),
            Request(
                "POST", _I + "/install/",
                route="/api/users/me/integrations/github/app/install/",
                json={"workspace_slug": "{ws}"},
                auth="member", expect_status=403,
            ),
            Request(
                "POST", _I + "/install/",
                route="/api/users/me/integrations/github/app/install/",
                json={"workspace_slug": "{ws}"},
                auth="admin",
                capture={"istate": "state"},
                body_regex_blanks=[
                    r"installations/new\?state=[A-Za-z0-9_-]+",
                ],
                expect_status=201,
            ),
            # App refresh: missing slug 400 -> unknown ws 404 ->
            # member 403 -> not-installed 404. (The live-check 502's
            # error text is client-defined per backend — the suite
            # pins it loosely for the same reason.)
            Request(
                "POST", _I + "/refresh/",
                route="/api/users/me/integrations/github/app/refresh/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _I + "/refresh/",
                route="/api/users/me/integrations/github/app/refresh/",
                json={"workspace_slug": "no-such-workspace"},
                auth="admin", expect_status=404,
            ),
            Request(
                "POST", _I + "/refresh/",
                route="/api/users/me/integrations/github/app/refresh/",
                json={"workspace_slug": "{ws}"},
                auth="member", expect_status=403,
            ),
            Request(
                "POST", _I + "/refresh/",
                route="/api/users/me/integrations/github/app/refresh/",
                json={"workspace_slug": "{ws}"},
                auth="admin", expect_status=404,
            ),
            # App webhook (HMAC-signed raw bodies): bad signature 401
            # -> missing delivery 400 -> bad delivery uuid 400 -> bad
            # JSON 400 -> ping 202 -> ping redelivery (idempotent) ->
            # unlinked PR skipped -> unknown installation skipped ->
            # unknown event skipped -> anonymous unsigned 401.
            Request(
                "POST", _WH + "/",
                route="/api/integrations/github/app/webhook/",
                raw_body=_BADSIG_BODY,
                headers={
                    "Content-Type": "application/json",
                    "X-Hub-Signature-256": "sha256=dead",
                    "X-GitHub-Event": "ping",
                    "X-GitHub-Delivery": _D1,
                },
                auth="admin", expect_status=401,
            ),
            Request(
                "POST", _WH + "/",
                route="/api/integrations/github/app/webhook/",
                raw_body=json.dumps({"zen": "x"}).encode(),
                headers={
                    "Content-Type": "application/json",
                    "X-Hub-Signature-256": _sign(json.dumps({"zen": "x"}).encode()),
                    "X-GitHub-Event": "ping",
                },
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _WH + "/",
                route="/api/integrations/github/app/webhook/",
                raw_body=json.dumps({"zen": "x"}).encode(),
                headers={
                    "Content-Type": "application/json",
                    "X-Hub-Signature-256": _sign(json.dumps({"zen": "x"}).encode()),
                    "X-GitHub-Event": "ping",
                    "X-GitHub-Delivery": "not-a-uuid",
                },
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _WH + "/",
                route="/api/integrations/github/app/webhook/",
                raw_body=_BAD_JSON_BODY,
                headers=_BAD_JSON_HEADERS,
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _WH + "/",
                route="/api/integrations/github/app/webhook/",
                raw_body=_PING_BODY,
                headers=_PING_HEADERS,
                auth="admin", expect_status=202,
            ),
            Request(
                "POST", _WH + "/",
                route="/api/integrations/github/app/webhook/",
                raw_body=_PING_BODY,
                headers=_PING_HEADERS,
                auth="admin", expect_status=202,
            ),
            Request(
                "POST", _WH + "/",
                route="/api/integrations/github/app/webhook/",
                raw_body=_PR_BODY,
                headers=_PR_HEADERS,
                auth="admin", expect_status=202,
            ),
            Request(
                "POST", _WH + "/",
                route="/api/integrations/github/app/webhook/",
                raw_body=_INSTALL_BODY,
                headers=_INSTALL_HEADERS,
                auth="admin", expect_status=202,
            ),
            Request(
                "POST", _WH + "/",
                route="/api/integrations/github/app/webhook/",
                raw_body=_STAR_BODY,
                headers=_STAR_HEADERS,
                auth="admin", expect_status=202,
            ),
            Request(
                "POST", _WH + "/",
                route="/api/integrations/github/app/webhook/",
                raw_body=b"{}",
                headers={"Content-Type": "application/json"},
                auth="anonymous", expect_status=401,
            ),
        ),
    ),
)
