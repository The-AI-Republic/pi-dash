# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Permission floor: anonymous denied everywhere admin-only; an authenticated
non-admin (another tenant's user) is denied the same endpoints while the
public console stays open. The suite must also fail when a permission class
is deliberately removed — demonstrated with a reverted one-line patch, not
a committed test.
"""

import pytest

from _harness import admin_client
from _harness.settings import secret_key as _secret

ADMIN_ENDPOINTS = [
    ("GET", "/api/instances/admins/"),
    ("POST", "/api/instances/admins/"),
    ("GET", "/api/instances/admins/me/"),
    ("GET", "/api/instances/configurations/"),
    ("PATCH", "/api/instances/configurations/"),
    ("DELETE", "/api/instances/configurations/disable-email-feature/"),
    ("POST", "/api/instances/email-credentials-check/"),
    ("GET", "/api/instances/workspace-slug-check/"),
    ("GET", "/api/instances/workspaces/"),
    ("POST", "/api/instances/workspaces/"),
    ("PATCH", "/api/instances/"),
    ("DELETE", "/api/instances/admins/00000000-0000-0000-0000-000000000000/"),
]


@pytest.mark.parametrize("method,path", ADMIN_ENDPOINTS)
def test_anonymous_is_denied(anon_api, world, method, path):
    # No session: DRF raises NotAuthenticated (401), not PermissionDenied.
    res = anon_api.request(method, path, json={})
    assert res.status_code == 401
    assert res.json() == {"detail": "Authentication credentials were not provided."}


@pytest.mark.parametrize("method,path", ADMIN_ENDPOINTS)
def test_non_admin_member_is_denied(member_api, method, path):
    res = member_api.request(method, path, json={})
    assert res.status_code == 403
    assert res.json() == {"detail": "You do not have permission to perform this action."}


def test_low_role_admin_is_denied(world):
    junior = world["db"].make_user("junior@example.com")
    world["db"].make_admin(world["instance"]["id"], junior["id"], role=5)
    with admin_client(world["db"].mint_admin_session(junior, _secret())) as client:
        assert client.get("/api/instances/admins/").status_code == 403


def test_public_console_stays_open_for_non_admin(member_api, anon_api):
    assert member_api.get("/api/instances/").status_code == 200
    assert anon_api.get("/api/instances/").status_code == 200
    assert anon_api.get("/api/instances/admins/session/").status_code == 200
