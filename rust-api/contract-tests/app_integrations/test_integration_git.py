"""Contract tests: generic git provider accounts + project repository binding.

Paths (``app/urls/integration.py``):
  GET    /api/workspaces/<slug>/integrations/git/providers/
  GET    /api/workspaces/<slug>/integrations/git/accounts/
  POST   /api/workspaces/<slug>/integrations/git/accounts/
  GET    /api/workspaces/<slug>/integrations/git/accounts/<id>/
  DELETE /api/workspaces/<slug>/integrations/git/accounts/<id>/
  GET    /api/workspaces/<slug>/integrations/git/accounts/<id>/repos/
  GET    /api/workspaces/<slug>/projects/<id>/repository/
  POST   /api/workspaces/<slug>/projects/<id>/repository/bind/
  PATCH  /api/workspaces/<slug>/projects/<id>/repository/
  DELETE /api/workspaces/<slug>/projects/<id>/repository/

Account creation and repo listing call the provider upstream, so the suite
pins the validation shapes plus the full detail/revoke lifecycle on
SQL-seeded ``git_provider_accounts`` rows. Repository bind verification is
likewise upstream; the suite pins its offline validation and the
unbound-state shapes.
"""

from __future__ import annotations

import pytest

from .conftest import ANON, DENIED, proj_url, ws_url

pytestmark = pytest.mark.contract

NIL = "00000000-0000-0000-0000-000000000000"


def test_providers_shape(admin, world):
    response = admin.get(ws_url(world, "integrations", "git", "providers"))
    assert response.status_code == 200
    assert response.json() == {
        "providers": [
            {"key": "github", "display_name": "GitHub", "code_review_term": "pull request"},
            {"key": "gitlab", "display_name": "GitLab", "code_review_term": "merge request"},
        ]
    }


def test_providers_guest_denied(guest_client, world):
    response = guest_client.get(ws_url(world, "integrations", "git", "providers"))
    assert response.status_code == 403
    assert response.json() == DENIED


def test_accounts_empty(admin, world):
    response = admin.get(ws_url(world, "integrations", "git", "accounts"))
    assert response.status_code == 200
    assert response.json() == {"accounts": []}


def test_account_create_rejects_unknown_provider(admin, world):
    response = admin.post(
        ws_url(world, "integrations", "git", "accounts"),
        json={"provider": "bitbucket", "token": "x"},
    )
    assert response.status_code == 400
    assert response.json() == {"error": "provider must be github or gitlab"}


def test_account_create_requires_token(admin, world):
    response = admin.post(
        ws_url(world, "integrations", "git", "accounts"), json={"provider": "github"}
    )
    assert response.status_code == 400
    assert response.json() == {"error": "token is required"}


def test_account_create_member_denied(member_client, world):
    response = member_client.post(
        ws_url(world, "integrations", "git", "accounts"),
        json={"provider": "github", "token": "x"},
    )
    assert response.status_code == 403
    assert response.json() == DENIED


def test_account_detail_unknown(admin, world):
    response = admin.get(ws_url(world, "integrations", "git", "accounts", NIL))
    assert response.status_code == 404
    assert response.json() == {"detail": "No GitProviderAccount matches the given query."}


def test_account_detail_shape(admin, world, seeder):
    account = seeder.create_git_provider_account(world["workspace"]["id"])
    response = admin.get(ws_url(world, "integrations", "git", "accounts", account["id"]))
    assert response.status_code == 200
    body = response.json()
    assert body["id"] == account["id"]
    assert body["provider"] == "github"
    assert body["host_url"] == "https://github.com"
    assert body["auth_type"] == "pat"
    assert body["external_account_login"] == "contract-octocat"
    assert body["display_name"] == "contract-octocat"
    assert body["capabilities"] == {}
    assert body["status"] == "connected"
    assert body["verified_at"] is not None
    assert body["last_check_error"] == ""
    assert set(body) == {
        "id",
        "provider",
        "host_url",
        "auth_type",
        "external_account_id",
        "external_account_login",
        "display_name",
        "capabilities",
        "status",
        "verified_at",
        "last_check_error",
    }


def test_account_cross_tenant_denied(other_admin, world, seeder):
    account = seeder.create_git_provider_account(world["workspace"]["id"])
    response = other_admin.get(
        ws_url(world, "integrations", "git", "accounts", account["id"])
    )
    assert response.status_code == 403
    assert response.json() == DENIED


def test_account_revoke(admin, world, seeder):
    account = seeder.create_git_provider_account(world["workspace"]["id"])
    response = admin.delete(
        ws_url(world, "integrations", "git", "accounts", account["id"])
    )
    assert response.status_code == 200
    assert response.json() == {"connected": False}

    detail = admin.get(ws_url(world, "integrations", "git", "accounts", account["id"]))
    assert detail.json()["status"] == "revoked"


def test_account_revoke_member_denied(member_client, world, seeder):
    account = seeder.create_git_provider_account(world["workspace"]["id"])
    response = member_client.delete(
        ws_url(world, "integrations", "git", "accounts", account["id"])
    )
    assert response.status_code == 403
    assert response.json() == DENIED


def test_account_repos_unknown_account(admin, world):
    response = admin.get(ws_url(world, "integrations", "git", "accounts", NIL, "repos"))
    assert response.status_code == 404


def test_project_repository_unbound(admin, world):
    response = admin.get(proj_url(world, "repository"))
    assert response.status_code == 200
    assert response.json() == {"bound": False}


def test_project_repository_guest_reads(guest_client, world):
    response = guest_client.get(proj_url(world, "repository"))
    assert response.status_code == 200
    assert response.json() == {"bound": False}


def test_project_repository_bind_validations(admin, world):
    assert admin.post(proj_url(world, "repository", "bind"), json={}).json() == {
        "error": "repo_url is required"
    }
    bad = admin.post(
        proj_url(world, "repository", "bind"), json={"repo_url": "https://bitbucket.org/o/r"}
    )
    assert bad.status_code == 400
    assert bad.json() == {"error": "A supported GitHub or GitLab repository URL is required"}


def test_project_repository_bind_member_denied(member_client, world):
    response = member_client.post(
        proj_url(world, "repository", "bind"),
        json={"repo_url": "https://github.com/o/r"},
    )
    assert response.status_code == 403
    assert response.json() == DENIED


def test_project_repository_toggle_validations(admin, world):
    bad = admin.patch(proj_url(world, "repository"), json={"enabled": "yes"})
    assert bad.status_code == 400
    assert bad.json() == {"error": "enabled must be boolean"}

    missing = admin.patch(proj_url(world, "repository"), json={"enabled": True})
    assert missing.status_code == 404
    assert missing.json() == {"error": "Repository is not bound"}


def test_project_repository_unbind(admin, world):
    response = admin.delete(proj_url(world, "repository"))
    assert response.status_code == 200
    assert response.json() == {"bound": False}


def test_project_repository_toggle_member_denied(member_client, world):
    response = member_client.patch(proj_url(world, "repository"), json={"enabled": True})
    assert response.status_code == 403
    assert response.json() == DENIED


def test_anonymous_rejected(anon, world):
    for response in (
        anon.get(ws_url(world, "integrations", "git", "providers")),
        anon.get(ws_url(world, "integrations", "git", "accounts")),
        anon.get(proj_url(world, "repository")),
    ):
        assert response.status_code == 401
        assert response.json() == ANON


def test_outsider_denied(outsider_client, world):
    response = outsider_client.get(ws_url(world, "integrations", "git", "providers"))
    assert response.status_code == 403
    assert response.json() == DENIED
