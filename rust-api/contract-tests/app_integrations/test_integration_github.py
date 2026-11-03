"""Contract tests: workspace GitHub PAT integration (connect/disconnect/status/repos).

Paths (``app/urls/integration.py``):
  GET  /api/workspaces/<slug>/integrations/github/
  POST /api/workspaces/<slug>/integrations/github/connect/
  POST /api/workspaces/<slug>/integrations/github/disconnect/
  GET  /api/workspaces/<slug>/integrations/github/repos/

The live-GitHub paths are pinned through their deterministic error shapes
(``connect`` with a bogus token is rejected upstream with 401); the
connected-state shapes come from SQL-seeded ``workspace_integrations`` rows so
no test needs a real credential.
"""

from __future__ import annotations

import pytest

from .conftest import ANON, DENIED, ws_url

pytestmark = pytest.mark.contract


def test_status_not_connected_shape(admin, world):
    response = admin.get(ws_url(world, "integrations", "github"))
    assert response.status_code == 200
    assert response.json() == {"connected": False}


def test_status_connected_shape(admin, world, seeder):
    seeder.create_workspace_integration(
        world["workspace"]["id"], world["owner"]["id"], connected=True
    )
    response = admin.get(ws_url(world, "integrations", "github"))
    assert response.status_code == 200
    assert response.json() == {
        "connected": True,
        "github_user_login": "contract-octocat",
        "verified_at": "2026-01-01T00:00:00+00:00",
    }


def test_status_member_reads(member_client, world):
    response = member_client.get(ws_url(world, "integrations", "github"))
    assert response.status_code == 200
    assert response.json() == {"connected": False}


def test_status_guest_denied(guest_client, world):
    response = guest_client.get(ws_url(world, "integrations", "github"))
    assert response.status_code == 403
    assert response.json() == DENIED


def test_connect_missing_token(admin, world):
    response = admin.post(ws_url(world, "integrations", "github", "connect"), json={})
    assert response.status_code == 400
    assert response.json() == {"error": "GitHub PAT required"}


def test_connect_bogus_token_rejected_upstream(admin, world):
    response = admin.post(
        ws_url(world, "integrations", "github", "connect"),
        json={"token": "ghp_boguscontracttesttoken0000000000"},
    )
    assert response.status_code == 401
    assert response.json() == {"error": "GitHub rejected this token"}


def test_connect_member_denied(member_client, world):
    response = member_client.post(
        ws_url(world, "integrations", "github", "connect"), json={"token": "x"}
    )
    assert response.status_code == 403
    assert response.json() == DENIED


def test_disconnect_never_connected(admin, world):
    response = admin.post(ws_url(world, "integrations", "github", "disconnect"))
    assert response.status_code == 200
    assert response.json() == {"connected": False}


def test_disconnect_connected_clears_credential(admin, world, seeder):
    seeder.create_workspace_integration(
        world["workspace"]["id"], world["owner"]["id"], connected=True
    )
    response = admin.post(ws_url(world, "integrations", "github", "disconnect"))
    assert response.status_code == 200
    assert response.json() == {"connected": False}

    status = admin.get(ws_url(world, "integrations", "github"))
    assert status.json() == {"connected": False}

    repos = admin.get(ws_url(world, "integrations", "github", "repos"))
    assert repos.status_code == 409
    assert repos.json() == {"error": "GitHub credential is missing"}


def test_disconnect_member_denied(member_client, world):
    response = member_client.post(ws_url(world, "integrations", "github", "disconnect"))
    assert response.status_code == 403
    assert response.json() == DENIED


def _install_revoke_blocker(db, tag: str, workspace_id: str) -> str:
    """A BEFORE UPDATE trigger on ``git_provider_accounts`` that raises.

    Scoped to the test's workspace via the WHEN clause so no other row can
    trip it. Returns the object name for teardown. Mirrors the runner
    self-delete atomicity pin (PIDASHCONV-1265 class).
    """
    fn = f"ct_ghws_{tag}"
    db.execute(
        f"CREATE OR REPLACE FUNCTION {fn}() RETURNS trigger AS $$ "
        f"BEGIN RAISE EXCEPTION 'contract: forced PAT revoke failure'; "
        f"END $$ LANGUAGE plpgsql"
    )
    db.execute(
        f"CREATE TRIGGER {fn} BEFORE UPDATE ON git_provider_accounts "
        f"FOR EACH ROW WHEN (NEW.workspace_id::text = '{workspace_id}') "
        f"EXECUTE FUNCTION {fn}()"
    )
    return fn


def _drop_revoke_blocker(db, fn: str) -> None:
    db.execute(f"DROP TRIGGER IF EXISTS {fn} ON git_provider_accounts")
    db.execute(f"DROP FUNCTION IF EXISTS {fn}()")


def test_disconnect_rolls_back_when_account_revoke_fails(admin, world, seeder):
    # PIDASHCONV-1467: Django wraps disconnect in transaction.atomic()
    # (github.py:512-536), so a failure in the final PAT revoke rolls the
    # config clear, sync and binding updates back too. Rust ran the same
    # writes as autocommit statements: the integration read "disconnected"
    # while the PAT stayed connected with its token intact.
    ws_id = world["workspace"]["id"]
    owner_id = world["owner"]["id"]
    wi = seeder.create_workspace_integration(ws_id, owner_id, connected=True)
    account = seeder.create_git_provider_account(ws_id)
    seeder.db.execute(
        "UPDATE git_provider_accounts SET workspace_integration_id = %s,"
        " credential_config = %s::jsonb WHERE id = %s",
        (
            wi["id"],
            '{"auth_type": "pat", "host_url": "https://github.com",'
            ' "token": "contract91-live-token"}',
            account["id"],
        ),
    )
    repo = seeder.create_git_repository()
    binding = seeder.create_git_binding(
        ws_id, world["project"]["id"], owner_id, repo["id"], account["id"]
    )
    seeder.db.execute(
        "UPDATE git_repository_bindings SET is_sync_enabled = true WHERE id = %s",
        (binding["id"],),
    )
    sync = seeder.create_github_repo_sync(ws_id, world["project"]["id"], owner_id, wi["id"])
    seeder.db.execute(
        "UPDATE github_repository_syncs SET is_sync_enabled = true WHERE id = %s",
        (sync["id"],),
    )

    fn = _install_revoke_blocker(seeder.db, seeder.tag, ws_id)
    try:
        response = admin.post(ws_url(world, "integrations", "github", "disconnect"))
    finally:
        _drop_revoke_blocker(seeder.db, fn)

    assert response.status_code == 500, response.text[:300]

    # All-or-nothing: the config token is unchanged and nothing else moved.
    config = seeder.db.fetchone(
        "SELECT config FROM workspace_integrations WHERE id = %s", (wi["id"],)
    )["config"]
    assert config["token"] == "contract91-seeded-token"
    assert "disconnected_at" not in config
    row = seeder.db.fetchone(
        "SELECT status, credential_config FROM git_provider_accounts WHERE id = %s",
        (account["id"],),
    )
    assert row["status"] == "connected"
    assert row["credential_config"]["token"] == "contract91-live-token"
    assert (
        seeder.db.fetchval(
            "SELECT is_sync_enabled FROM git_repository_bindings WHERE id = %s",
            (binding["id"],),
        )
        is True
    )
    assert (
        seeder.db.fetchval(
            "SELECT is_sync_enabled FROM github_repository_syncs WHERE id = %s",
            (sync["id"],),
        )
        is True
    )


def test_repos_not_connected(admin, world):
    response = admin.get(ws_url(world, "integrations", "github", "repos"))
    assert response.status_code == 404
    assert response.json() == {"error": "GitHub not connected"}


def test_repos_credential_missing_after_row_without_token(admin, world, seeder):
    seeder.create_workspace_integration(world["workspace"]["id"], world["owner"]["id"])
    response = admin.get(ws_url(world, "integrations", "github", "repos"))
    assert response.status_code == 409
    assert response.json() == {"error": "GitHub credential is missing"}


def test_repos_member_denied_without_membership(outsider_client, world):
    response = outsider_client.get(ws_url(world, "integrations", "github", "repos"))
    assert response.status_code == 403
    assert response.json() == DENIED


def test_anonymous_rejected(anon, world):
    for response in (
        anon.get(ws_url(world, "integrations", "github")),
        anon.post(ws_url(world, "integrations", "github", "connect"), json={}),
        anon.post(ws_url(world, "integrations", "github", "disconnect")),
        anon.get(ws_url(world, "integrations", "github", "repos")),
    ):
        assert response.status_code == 401
        assert response.json() == ANON
