# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Renamed-repo bind: GitHub 301 followed, GitLab 3xx fails closed (PIDASHCONV-1513).

Bug: the Rust repo-bind handler built one shared reqwest client with
``Policy::none()`` for both providers, but Django's ``GithubClient``
follows redirects (``requests.request`` default) while its GitLab client
opts out (``allow_redirects=False``). Binding a renamed/transferred
GitHub repo by its old owner/name 400d in Rust (``301 Moved Permanently``
→ ``ProviderError::Other``) where Django follows the 301 and binds. The
fix splits the client per provider (GitHub ``Policy::limited(10)``,
GitLab ``Policy::none()``).

How the stub reaches each backend (PIDASHCONV-1472 pattern, serve
variant): both backends hardcode https://api.github.com on the bind
path, so each leg uses its own mechanism over one shared route table
(:func:`bind_redirect_routes`):

- Django leg (this module): the server boots with ``HTTPS_PROXY``
  pointing at :class:`_harness.sinks.MitmProxy` (fixed
  ``BIND_REDIRECT_MITM_PORT``) and ``REQUESTS_CA_BUNDLE`` at a bundle
  this module writes (test CA minted per run + system roots) to
  ``BIND_REDIRECT_CA_BUNDLE``. The proxy MITMs api.github.com and
  serves the 301 routes in-tunnel; anything else is blind-relayed. No
  Python file changed.
- Rust leg (replay): the server boots with ``PIDASH_GITHUB_API_BASE``
  pointing at a plain-HTTP :class:`_harness.sinks.RedirectStub` (fixed
  ``BIND_REDIRECT_STUB_PORT``), which rustls never has to verify. That
  variable is the only product-code addition, unset in production.

The GitLab leg reuses the same MITM: the seeded account carries
``host_url=https://api.github.com`` (added to the server's
``GITLAB_ALLOWED_HOSTS``), so the GitLab client's project GET goes
through the proxy and its 301 fails closed without following. The
hostname is incidental — what the test pins is the client's
``allow_redirects=False`` behavior. Rust has no GitLab stub seam
(https required, rustls pins Mozilla roots), so the Rust side of that
guard is the ``gitlab_client_surfaces_redirect_fail_closed`` unit test
and the replay module covers the GitHub leg only.

Each test seeds its own world + PAT account (Fernet ciphertext under the
server's SECRET_KEY, production shape) and deletes the server-created
binding rows on teardown. Running against a server without the stub env
skips every test (loop-membership gate on fresh MITM hits).
"""

from __future__ import annotations

import json
import os
import uuid

import pytest

from _harness import sinks
from integrations import seed as int_seed

from .conftest import proj_url

pytestmark = pytest.mark.contract

OLD_OWNER = "bind-oldowner"
OLD_NAME = "bind-oldrepo"
NEW_OWNER = "bind-newowner"
NEW_NAME = "bind-newrepo"
REPO_ID = 987654321
GL_GROUP = "glgroup"
GL_PROJ = "glproj"
PLAIN_PAT = "bind-redirect-pat-1513"

STUB_PORT = int(os.environ.get("BIND_REDIRECT_STUB_PORT", "15131"))
MITM_PORT = int(os.environ.get("BIND_REDIRECT_MITM_PORT", "15132"))
CA_BUNDLE_PATH = os.environ.get(
    "BIND_REDIRECT_CA_BUNDLE", "/tmp/pidash-1513-ca-bundle.pem"
)

REPO_PAYLOAD = {
    "id": REPO_ID,
    "name": NEW_NAME,
    "full_name": f"{NEW_OWNER}/{NEW_NAME}",
    "owner": {"login": NEW_OWNER},
    "html_url": f"https://github.com/{NEW_OWNER}/{NEW_NAME}",
    "clone_url": f"https://github.com/{NEW_OWNER}/{NEW_NAME}.git",
    "ssh_url": f"git@github.com:{NEW_OWNER}/{NEW_NAME}.git",
    "default_branch": "main",
    "private": False,
}

REPOSITORY_SHAPE = {
    "id": str(REPO_ID),
    "provider": "github",
    "host_url": "https://github.com",
    "namespace": NEW_OWNER,
    "name": NEW_NAME,
    "full_name": f"{NEW_OWNER}/{NEW_NAME}",
    "web_url": f"https://github.com/{NEW_OWNER}/{NEW_NAME}",
    "clone_url_http": f"https://github.com/{NEW_OWNER}/{NEW_NAME}.git",
    "clone_url_ssh": f"git@github.com:{NEW_OWNER}/{NEW_NAME}.git",
    "default_branch": "main",
    "private": False,
}

BINDING_KEYS = {
    "bound",
    "id",
    "provider",
    "provider_account_id",
    "host_url",
    "repository",
    "is_sync_enabled",
    "clone_auth_mode",
    "last_synced_at",
    "last_sync_error",
    "degraded",
    "degraded_reason",
    "repo_url",
}


def bind_redirect_routes() -> dict:
    """Shared stub routes: old repo path 301s, GitLab project path 301s.

    ``{base}`` formats to the serving leg's own base (the MITM's
    https://api.github.com or the plain stub's http://127.0.0.1:port), so
    the GitHub client follows same-host Locations. The 301 target mirrors
    real GitHub, which redirects renamed repos to /repositories/{id}.
    """
    old_repo = f"/repos/{OLD_OWNER}/{OLD_NAME}"
    return {
        ("GET", old_repo): (
            301,
            b"",
            {"Location": "{base}/repositories/987654321"},
        ),
        ("GET", "/repositories/987654321"): (200, REPO_PAYLOAD),
        ("GET", f"/api/v4/projects/{GL_GROUP}%2F{GL_PROJ}"): (
            301,
            b"",
            {"Location": "{base}/api/v4/projects/12345"},
        ),
    }


def _secret_key() -> str:
    value = os.environ.get("SECRET_KEY")
    if not value:
        raise pytest.skip("SECRET_KEY is not set — must match the server's")
    return value


def _store_token(seeder, account_id: str, *, host_url: str | None = None) -> None:
    """Store a production-shape Fernet ciphertext token on the account."""
    token = int_seed.encrypt_token_for_worker(PLAIN_PAT, _secret_key())
    assert token and token != PLAIN_PAT
    if host_url is None:
        seeder.db.execute(
            "UPDATE git_provider_accounts SET credential_config = %s WHERE id = %s",
            (json.dumps({"token": token}), account_id),
        )
    else:
        seeder.db.execute(
            "UPDATE git_provider_accounts SET host_url = %s, credential_config = %s"
            " WHERE id = %s",
            (host_url, json.dumps({"token": token}), account_id),
        )


def cleanup_binding_rows(db, project_id: str) -> None:
    """Delete server-created bind rows the seed tracker cannot see."""
    db.execute(
        "DELETE FROM git_repository_bindings WHERE project_id = %s", (project_id,)
    )
    db.execute(
        "DELETE FROM git_repositories gr WHERE gr.external_id = %s AND gr.provider = 'github'"
        " AND NOT EXISTS (SELECT 1 FROM git_repository_bindings b"
        " WHERE b.repository_id = gr.id)",
        (str(REPO_ID),),
    )


def hits_matching(entries: list[dict], method: str, prefix: str) -> list[dict]:
    """Hits with ``method`` whose path starts with ``prefix`` (superset match).

    Transport-agnostic over MITM (Django leg) and RedirectStub (Rust leg)
    snapshots: CONNECT entries never match a GET/POST method, so no flag
    filtering is needed.
    """
    return [
        entry
        for entry in entries
        if entry["method"] == method and entry["path"].startswith(prefix)
    ]


def assert_bind_renamed_ok(body: dict, account_id: str) -> None:
    """The 201 binds the RENAMED repo (redirect target's metadata)."""
    assert set(body) == BINDING_KEYS, set(body) ^ BINDING_KEYS
    uuid.UUID(body["id"])
    assert body["bound"] is True
    assert body["provider"] == "github"
    assert body["provider_account_id"] == account_id
    assert body["host_url"] == "https://github.com"
    assert body["repository"] == REPOSITORY_SHAPE
    assert body["is_sync_enabled"] is False
    assert body["clone_auth_mode"] == "public"
    assert body["last_synced_at"] is None
    assert body["last_sync_error"] == ""
    assert body["degraded"] is False
    assert body["degraded_reason"] == ""
    assert body["repo_url"] == REPO_PAYLOAD["clone_url"]


@pytest.fixture(scope="module")
def redirect_mitm(tmp_path_factory):
    """MITM proxy on the fixed port + freshly minted CA bundle.

    The bundle publishes atomically to the fixed path the server was
    booted with; the server reads it per connection, and makes no
    api.github.com request before the first test binds.
    """
    bundle, cert, key = sinks.make_ca_bundle(tmp_path_factory.mktemp("mitm-ca"))
    os.replace(bundle, CA_BUNDLE_PATH)
    with sinks.MitmProxy(
        bind_redirect_routes(), tls_cert=cert, tls_key=key, port=MITM_PORT
    ) as proxy:
        yield proxy


@pytest.fixture()
def bind_world(world, db):
    yield world
    cleanup_binding_rows(db, world["project"]["id"])


def _gate_on_mitm(fresh: list[dict]) -> None:
    """Skip when the server never came through this module's MITM."""
    if not any(
        entry["method"] == "CONNECT"
        and entry["path"].split(":")[0].lower() == "api.github.com"
        for entry in fresh
    ):
        raise pytest.skip(
            "Server is not routed through the bind-redirect MITM — boot it"
            f" with HTTPS_PROXY=http://127.0.0.1:{MITM_PORT} and"
            f" REQUESTS_CA_BUNDLE={CA_BUNDLE_PATH}"
        )


def test_bind_renamed_github_repo_follows_301(
    admin, bind_world, seeder, db, redirect_mitm
):
    world = bind_world
    account = seeder.create_git_provider_account(world["workspace"]["id"], provider="github")
    _store_token(seeder, account["id"])
    url = proj_url(world, "repository", "bind")
    before = len(redirect_mitm.snapshot())
    response = admin.post(url, json={"repo_url": f"https://github.com/{OLD_OWNER}/{OLD_NAME}"})
    fresh = redirect_mitm.snapshot()[before:]
    _gate_on_mitm(fresh)
    assert response.status_code == 201, response.text[:500]
    assert_bind_renamed_ok(response.json(), account["id"])
    # The redirect hop itself, not just the outcome.
    assert hits_matching(fresh, "GET", f"/repos/{OLD_OWNER}/{OLD_NAME}"), "stub never saw the old path"
    assert hits_matching(fresh, "GET", "/repositories/987654321"), "stub never saw the followed hop"
    # The bind row carries the renamed repo and stamps the branch.
    row = db.fetchone(
        "SELECT r.external_id, r.full_name, p.base_branch, p.repo_url"
        " FROM git_repository_bindings b"
        " JOIN git_repositories r ON r.id = b.repository_id"
        " JOIN projects p ON p.id = b.project_id"
        " WHERE b.project_id = %s",
        (world["project"]["id"],),
    )
    assert row is not None, "bind row missing"
    assert row["external_id"] == str(REPO_ID), row
    assert row["full_name"] == f"{NEW_OWNER}/{NEW_NAME}", row
    assert row["base_branch"] == "main", row
    assert row["repo_url"] == REPO_PAYLOAD["clone_url"], row


def test_bind_gitlab_redirect_fails_closed(admin, bind_world, seeder, redirect_mitm):
    world = bind_world
    account = seeder.create_git_provider_account(world["workspace"]["id"], provider="gitlab")
    _store_token(seeder, account["id"], host_url="https://api.github.com")
    url = proj_url(world, "repository", "bind")
    before = len(redirect_mitm.snapshot())
    response = admin.post(url, json={"repo_url": f"https://api.github.com/{GL_GROUP}/{GL_PROJ}"})
    fresh = redirect_mitm.snapshot()[before:]
    _gate_on_mitm(fresh)
    assert response.status_code == 403, response.text[:500]
    assert response.json() == {"error": "GitLab API redirects are not followed"}
    # Exactly one upstream hit: the 301 is surfaced, never followed.
    tunneled = [entry for entry in fresh if entry.get("tunneled")]
    assert len(tunneled) == 1, [(entry["method"], entry["path"]) for entry in fresh]
    assert tunneled[0]["method"] == "GET"
    assert tunneled[0]["path"].startswith(f"/api/v4/projects/{GL_GROUP}%2F{GL_PROJ}")
