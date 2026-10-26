# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Rust replay of the rename-bind redirect oracle (PIDASHCONV-1513).

The GitHub case from ``test_repo_bind_redirect.py``, driven through the
Rust server instead of Django. The server boots with
``PIDASH_GITHUB_API_BASE`` pointing at the shared route table served
over plain HTTP (fixed ``BIND_REDIRECT_STUB_PORT``); the Django leg
reaches the same routes through its MITM proxy instead. Payloads,
routes and assertions are imported from the oracle module; only the
transport (this module's stub + hit gate) differs.

No GitLab replay: the GitLab client's stub seam is its credential
``host_url``, which must be https on both backends, and the Rust
client pins Mozilla roots with no test-CA override — so no stub can
answer the Rust GitLab fetch. The Django-leg GitLab test pins the
fail-closed contract; the Rust side is pinned by the
``gitlab_client_surfaces_redirect_fail_closed`` unit test.

Run with the Rust server up against its own DATABASE_URL (dedicated CI
job). A Rust server without the override env hits real GitHub and
records 401s, so the loop-membership gate skips instead of failing when
the stub saw no fresh hit.
"""

from __future__ import annotations

import pytest

from _harness import sinks

from . import test_repo_bind_redirect as oracle
from .conftest import proj_url

pytestmark = pytest.mark.contract


@pytest.fixture(scope="module")
def redirect_stub():
    """Plain-HTTP redirect stub on the fixed port the server dials."""
    with sinks.RedirectStub(oracle.bind_redirect_routes(), port=oracle.STUB_PORT) as stub:
        yield stub


@pytest.fixture()
def bind_world(world, db):
    yield world
    oracle.cleanup_binding_rows(db, world["project"]["id"])


def test_rust_bind_renamed_github_repo_follows_301(
    admin, bind_world, seeder, db, redirect_stub
):
    world = bind_world
    account = seeder.create_git_provider_account(world["workspace"]["id"], provider="github")
    oracle._store_token(seeder, account["id"])
    url = proj_url(world, "repository", "bind")
    before = len(redirect_stub.snapshot())
    response = admin.post(
        url, json={"repo_url": f"https://github.com/{oracle.OLD_OWNER}/{oracle.OLD_NAME}"}
    )
    fresh = redirect_stub.snapshot()[before:]
    if not fresh:
        raise pytest.skip(
            "Rust server is not pointed at the bind-redirect stub — boot it with"
            f" PIDASH_GITHUB_API_BASE=http://127.0.0.1:{oracle.STUB_PORT}"
        )
    assert response.status_code == 201, response.text[:500]
    oracle.assert_bind_renamed_ok(response.json(), account["id"])
    # The redirect hop itself, not just the outcome.
    assert oracle.hits_matching(fresh, "GET", f"/repos/{oracle.OLD_OWNER}/{oracle.OLD_NAME}"), (
        "stub never saw the old path"
    )
    assert oracle.hits_matching(fresh, "GET", "/repositories/987654321"), (
        "stub never saw the followed hop"
    )
    row = db.fetchone(
        "SELECT r.external_id, r.full_name, p.base_branch, p.repo_url"
        " FROM git_repository_bindings b"
        " JOIN git_repositories r ON r.id = b.repository_id"
        " JOIN projects p ON p.id = b.project_id"
        " WHERE b.project_id = %s",
        (world["project"]["id"],),
    )
    assert row is not None, "bind row missing"
    assert row["external_id"] == str(oracle.REPO_ID), row
    assert row["full_name"] == f"{oracle.NEW_OWNER}/{oracle.NEW_NAME}", row
    assert row["base_branch"] == "main", row
    assert row["repo_url"] == oracle.REPO_PAYLOAD["clone_url"], row
