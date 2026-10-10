# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""G6 contract proof: denied-method denials on the 7 workspace routes.

Parity gap G6 (PIDASHCONV-820, EXPECTED.md): the disallowed methods on
these routes are served by explicit ``*_not_allowed`` Rust handlers that
replay the view's auth + permission prelude and answer DRF's 405 bytes.
This suite pins the denial wire shape — status, exact body, ``Allow`` —
for every denied (method, path) x actor cell, against both backends.

Two subtleties the suite encodes (both verified against live Django):

1. HEAD on a GET route is NOT a denial. Django's ``View.setup``
   (``django/views/generic/base.py``) binds ``head = get``, so HEAD runs
   the GET handler (status 200/400/404 per route+actor, body stripped by
   the server). Only the POST-only export route truly 405s HEAD. The
   parity inventory misses the binding, so the G6 ``HEAD`` segments are
   inventory artifacts; ``test_head_serves`` pins the served behavior.
2. DRF's ``finalize_response`` stamps the view's ``Allow`` header on
   error responses too, so every 401/403/405 here carries one (HEAD
   appears in the value exactly where GET does — the same binding).
   Served-HEAD responses assert status only: ``Allow`` on owned-method
   responses is a separate global gap, out of scope for G6.
"""

import uuid

import pytest

from _harness import user_client
from _harness.settings import secret_key

BODY_401 = {"detail": "Authentication credentials were not provided."}
BODY_403 = {"detail": "You do not have permission to perform this action."}


def body_405(method):
    return {"detail": f'Method "{method}" not allowed.'}


# Per-view DRF Allow values (allowed_methods in http_method_names order;
# HEAD rides with GET via the setup() binding; OPTIONS always served).
ALLOW = {
    "slug-check": "GET, HEAD, OPTIONS",
    "workspaces": "GET, POST, HEAD, OPTIONS",
    "workspace": "GET, PUT, PATCH, DELETE, HEAD, OPTIONS",
    "mine": "GET, HEAD, OPTIONS",
    "themes": "GET, POST, HEAD, OPTIONS",
    "theme": "GET, PATCH, DELETE, HEAD, OPTIONS",
    "export": "POST, OPTIONS",
}

ALL_405 = {"admin": 405, "member": 405, "guest": 405, "outsider": 405}
ALL_403 = {"admin": 403, "member": 403, "guest": 403, "outsider": 403}
ADMIN_MEMBER_405 = {"admin": 405, "member": 405, "guest": 403, "outsider": 403}
SAFE_EXPORT_GET = {"admin": 405, "member": 405, "guest": 405, "outsider": 403}

# True denials: (route key, method) -> {actor: status}. Bodies derive from
# the status (405/403 exact JSON; HEAD has no body on the wire — the
# server strips it — so HEAD asserts status + Allow only).
DENIED = {
    "slug-check": {m: ALL_405 for m in ("POST", "PUT", "PATCH", "DELETE")},
    # Collection with slug=None: the class check denies every write.
    "workspaces": {m: ALL_403 for m in ("PUT", "PATCH", "DELETE")},
    "workspace": {"POST": ALL_405},
    "workspace-bogus": {"POST": ALL_405},
    "mine": {m: ALL_405 for m in ("POST", "PUT", "PATCH", "DELETE")},
    "themes": {m: ADMIN_MEMBER_405 for m in ("PUT", "PATCH", "DELETE")},
    "themes-bogus": {m: ALL_403 for m in ("PUT", "PATCH", "DELETE")},
    "theme": {m: ADMIN_MEMBER_405 for m in ("POST", "PUT")},
    # Entity permission with the actual method: safe needs any active
    # membership, writes need Admin/Member.
    "export": {
        "GET": SAFE_EXPORT_GET,
        "PUT": ADMIN_MEMBER_405,
        "PATCH": ADMIN_MEMBER_405,
        "DELETE": ADMIN_MEMBER_405,
        "HEAD": SAFE_EXPORT_GET,
    },
}

# Served HEAD (head=get binding): route key -> {actor: status}. Status
# only — see the module docstring on owned-response Allow.
SERVED_HEAD = {
    # Missing ?slug=: the GET handler's 400.
    "slug-check": {"admin": 400, "member": 400, "guest": 400, "outsider": 400},
    # Ported bug (test_list_workspaces_known_bug): the list action's
    # decorator reads kwargs["slug"] on the slugless route.
    "workspaces": {"admin": 400, "member": 400, "guest": 400, "outsider": 400},
    "workspace": {"admin": 200, "member": 200, "guest": 200, "outsider": 404},
    "workspace-bogus": {"admin": 404, "member": 404, "guest": 404, "outsider": 404},
    "mine": {"admin": 200, "member": 200, "guest": 200, "outsider": 200},
    "themes": {"admin": 200, "member": 200, "guest": 403, "outsider": 403},
    "theme": {"admin": 200, "member": 200, "guest": 403, "outsider": 403},
    "theme-bogus-pk": {"admin": 404, "member": 404, "guest": 403, "outsider": 403},
}


@pytest.fixture
def space(world):
    db = world["db"]
    ws = db.make_workspace("Acme", "acme", world["user"]["id"])
    db.make_workspace_member(ws["id"], world["user"]["id"], role=20)
    return ws


@pytest.fixture
def member_api(world, space):
    """other@example.com as a role-15 member of the space workspace."""
    world["db"].make_workspace_member(space["id"], world["other"]["id"], role=15)
    with user_client(world["other_key"]) as client:
        yield client


@pytest.fixture
def guest_api(world, space):
    """A dedicated role-5 guest of the space workspace."""
    db = world["db"]
    guest = db.make_user("guest@example.com", first_name="Guest", last_name="User")
    db.make_profile(guest["id"])
    db.make_workspace_member(space["id"], guest["id"], role=5)
    with user_client(db.mint_user_session(guest, secret_key())) as client:
        yield client


@pytest.fixture
def outsider_api(world):
    """A dedicated authenticated non-member (never joins any workspace)."""
    db = world["db"]
    outsider = db.make_user("outsider@example.com", first_name="Out", last_name="Sider")
    db.make_profile(outsider["id"])
    with user_client(db.mint_user_session(outsider, secret_key())) as client:
        yield client


@pytest.fixture
def theme_id(user_api, space):
    res = user_api.post(
        f"/api/workspaces/{space['slug']}/workspace-themes/",
        json={"name": "T1", "deleted_at": None},
    )
    assert res.status_code == 201
    return res.json()["id"]


@pytest.fixture
def actor_clients(user_api, member_api, guest_api, outsider_api):
    return {"admin": user_api, "member": member_api, "guest": guest_api, "outsider": outsider_api}


def denial_path(key, space, world, theme_pk):
    slug = space["slug"]
    uid = world["user"]["id"]
    return {
        "slug-check": "/api/workspace-slug-check/",
        "workspaces": "/api/workspaces/",
        "workspace": f"/api/workspaces/{slug}/",
        "workspace-bogus": "/api/workspaces/no-such-workspace/",
        "mine": "/api/users/me/workspaces/",
        "themes": f"/api/workspaces/{slug}/workspace-themes/",
        "themes-bogus": "/api/workspaces/no-such-workspace/workspace-themes/",
        "theme": f"/api/workspaces/{slug}/workspace-themes/{theme_pk}/",
        "export": f"/api/workspaces/{slug}/user-activity/{uid}/export/",
    }[key]


def served_head_path(key, space, theme_id):
    slug = space["slug"]
    return {
        "slug-check": "/api/workspace-slug-check/",
        "workspaces": "/api/workspaces/",
        "workspace": f"/api/workspaces/{slug}/",
        "workspace-bogus": "/api/workspaces/no-such-workspace/",
        "mine": "/api/users/me/workspaces/",
        "themes": f"/api/workspaces/{slug}/workspace-themes/",
        "theme": f"/api/workspaces/{slug}/workspace-themes/{theme_id}/",
        "theme-bogus-pk": f"/api/workspaces/{slug}/workspace-themes/{uuid.uuid4()}/",
    }[key]


def allow_for(key):
    return ALLOW[key.split("-bogus")[0]]


def denied_cases():
    for key, methods in sorted(DENIED.items()):
        for method, actors in sorted(methods.items()):
            for actor, status in sorted(actors.items()):
                yield pytest.param(key, method, actor, status, id=f"{key}-{method}-{actor}")


def test_denied_anon_401(anon_api, space, world):
    """Anonymous hits on every denied method answer 401 + Allow."""
    theme_pk = uuid.uuid4()
    for key, methods in sorted(DENIED.items()):
        for method in sorted(methods):
            res = anon_api.request(method, denial_path(key, space, world, theme_pk))
            assert res.status_code == 401, (key, method)
            assert res.headers["allow"] == allow_for(key), (key, method)
            if method != "HEAD":
                assert res.json() == BODY_401, (key, method)


@pytest.mark.parametrize("key,method,actor,expected", list(denied_cases()))
def test_denied_matrix(key, method, actor, expected, actor_clients, space, world):
    """Authenticated denials: exact status + body + Allow per cell."""
    theme_pk = uuid.uuid4()
    res = actor_clients[actor].request(method, denial_path(key, space, world, theme_pk))
    assert res.status_code == expected, (key, method, actor)
    assert res.headers["allow"] == allow_for(key), (key, method, actor)
    if method != "HEAD":
        want = body_405(method) if expected == 405 else BODY_403
        assert res.json() == want, (key, method, actor)


def test_head_anon_401(anon_api, space, theme_id):
    """Anonymous HEAD on served routes answers 401 (status only)."""
    for key in sorted(SERVED_HEAD):
        res = anon_api.head(served_head_path(key, space, theme_id))
        assert res.status_code == 401, key


def served_head_cases():
    for key, actors in sorted(SERVED_HEAD.items()):
        for actor, status in sorted(actors.items()):
            yield pytest.param(key, actor, status, id=f"{key}-{actor}")


@pytest.mark.parametrize("key,actor,expected", list(served_head_cases()))
def test_head_serves(key, actor, expected, actor_clients, space, theme_id):
    """HEAD on GET routes runs the GET handler (Django setup binding)."""
    res = actor_clients[actor].head(served_head_path(key, space, theme_id))
    assert res.status_code == expected, (key, actor)


def test_head_slug_check_with_query(user_api):
    """HEAD through the GET handler honors query params (200 here)."""
    res = user_api.head("/api/workspace-slug-check/", params={"slug": "acme"})
    assert res.status_code == 200
