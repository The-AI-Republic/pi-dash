# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Oracle: app vs spaces tenant behavior.

Recorded differences between the app and ``spaces/`` magic routes (per the
Python sources):

- Success redirects land on different bases: the app base for
  ``magic-sign-in/up``, the spaces base (``.../spaces``) for the ``spaces/``
  variants.
- ``next_path`` handling differs: the app routes echo it as a ``next_path``
  query parameter (``get_safe_redirect_url``); the spaces routes join it onto
  the redirect path (``validate_next_path`` + string concat).
- Magic tokens are NOT tenant-bound: a token generated through either
  generate route redeems on either sign-in route (shared ``magic_<email>``
  Redis namespace, same ``MagicCodeProvider``).
- Code-level (not directly observable here): only the app generate endpoint
  declares ``throttle_classes = [AuthenticationThrottle]``; the spaces one
  declares none. Both share the ``MagicCodeProvider`` attempt-exhaustion
  guard, which is what the max-attempt 400s pin.
"""

import pytest

from _harness import redis as redis_helper

from .conftest import (
    csrf_client,
    fresh_email,
    generate,
    location_path,
    location_query,
    post_form,
)

APP_SIGN_IN = "/auth/magic-sign-in/"
SPACE_SIGN_IN = "/auth/spaces/magic-sign-in/"
APP_GENERATE = "/auth/magic-generate/"
SPACE_GENERATE = "/auth/spaces/magic-generate/"


def _redeem(client, rdb, generate_path, sign_in_path, email: str):
    redis_helper.clear_magic(rdb, email)
    assert generate(client, generate_path, email).status_code == 200
    token = str(redis_helper.read_magic(rdb, email)["token"])
    return post_form(client, sign_in_path, {"email": email, "code": token})


@pytest.mark.contract
class TestTenantIsolation:
    def test_success_bases_differ(self, seed, rdb):
        user = seed.user(email=fresh_email())
        app_client = csrf_client()
        space_client = csrf_client()

        app_resp = _redeem(app_client, rdb, APP_GENERATE, APP_SIGN_IN, user["email"])
        space_resp = _redeem(space_client, rdb, SPACE_GENERATE, SPACE_SIGN_IN, user["email"])

        assert app_resp.status_code == 302, app_resp.text
        assert space_resp.status_code == 302, space_resp.text
        assert "error_code" not in location_query(app_resp)
        assert "error_code" not in location_query(space_resp)
        assert not location_path(app_resp).rstrip("/").endswith("/spaces")
        assert location_path(space_resp).rstrip("/").endswith("/spaces")

    def test_next_path_handling_differs(self, seed, rdb):
        user = seed.user(email=fresh_email())
        app_client = csrf_client()
        space_client = csrf_client()

        redis_helper.clear_magic(rdb, user["email"])
        assert generate(app_client, APP_GENERATE, user["email"]).status_code == 200
        app_token = str(redis_helper.read_magic(rdb, user["email"])["token"])
        redis_helper.clear_magic(rdb, user["email"])
        assert generate(space_client, SPACE_GENERATE, user["email"]).status_code == 200
        space_token = str(redis_helper.read_magic(rdb, user["email"])["token"])

        app_resp = post_form(
            app_client,
            APP_SIGN_IN,
            {"email": user["email"], "code": app_token, "next_path": "/workspaces"},
        )
        space_resp = post_form(
            space_client,
            SPACE_SIGN_IN,
            {"email": user["email"], "code": space_token, "next_path": "/workspaces"},
        )

        assert app_resp.status_code == 302, app_resp.text
        assert space_resp.status_code == 302, space_resp.text
        # App echoes next_path as a query parameter ...
        assert location_query(app_resp).get("next_path") == "/workspaces"
        # ... spaces joins it onto the redirect path.
        assert location_path(space_resp) == "/spaces/workspaces"

    @pytest.mark.parametrize(
        "generate_path,sign_in_path", [(APP_GENERATE, SPACE_SIGN_IN), (SPACE_GENERATE, APP_SIGN_IN)]
    )
    def test_token_redeems_cross_tenant(self, seed, rdb, generate_path, sign_in_path):
        user = seed.user(email=fresh_email())
        client = csrf_client()

        resp = _redeem(client, rdb, generate_path, sign_in_path, user["email"])

        assert resp.status_code == 302, resp.text
        assert "error_code" not in location_query(resp)
