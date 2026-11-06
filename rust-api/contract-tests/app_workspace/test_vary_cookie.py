# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""NEWPIDASH-56: session-authenticated responses carry Vary: Cookie.

Django's SessionMiddleware stamps ``Vary: Cookie`` on every response whose
session was read (``apps/api/pi_dash/authentication/middleware/session.py:
53-56``, ``django/contrib/sessions/middleware.py:29,45-46``). Every
session-authenticated API request reads ``_auth_user_id`` via
``AuthenticationMiddleware``/``get_user``, so every authenticated response
carries the header. The Rust handlers resolve the actor through
``SessionHandle::snapshot()`` (a clone), which historically marked
``accessed`` on the throw-away clone only, leaving the response without
``Vary: Cookie``. The fix makes any ``snapshot()`` read mark the live
session accessed.

Each case below is a tier 2 header assertion: ``Vary`` contains ``Cookie``
(case-insensitive, comma-split like ``append_vary``). Order and the other
``Vary`` fields (``Accept-Encoding``, ``origin``) are not pinned.
"""

import pytest


@pytest.fixture
def space(world):
    db = world["db"]
    ws = db.make_workspace("Acme", "acme", world["user"]["id"])
    db.make_workspace_member(ws["id"], world["user"]["id"], role=20)
    return ws


def _vary_fields(response):
    raw = response.headers.get("vary", "")
    return [v.strip().lower() for v in raw.split(",") if v.strip()]


def test_me_workspaces_varies_on_cookie(user_api):
    res = user_api.get("/api/users/me/workspaces/")
    assert res.status_code == 200
    assert "cookie" in _vary_fields(res)


def test_workspace_projects_varies_on_cookie(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/projects/")
    assert res.status_code == 200
    assert "cookie" in _vary_fields(res)


def test_workspace_user_favorites_varies_on_cookie(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/user-favorites/")
    assert res.status_code == 200
    assert "cookie" in _vary_fields(res)
