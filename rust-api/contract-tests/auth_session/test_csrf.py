# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""GET /auth/get-csrf-token/ (DRF APIView, AllowAny, 200 {"csrf_token"}) and
the Origin verification every auth CSRF gate runs first (NEWPIDASH-12)."""

import httpx
import pytest

from _harness.checks import require_keys
from _harness.settings import database_url, secret_key

from . import tokens
from .conftest import NEW_PASSWORD, PASSWORD, SESSION_COOKIE_NAME, login_cookies


def test_shape(anon):
    res = anon.get("/auth/get-csrf-token/")
    assert res.status_code == 200
    body = res.json()
    require_keys(body, ["csrf_token"], ctx="get-csrf-token")
    assert isinstance(body["csrf_token"], str) and body["csrf_token"], body


def test_sets_csrf_cookie(anon):
    res = anon.get("/auth/get-csrf-token/")
    assert res.status_code == 200
    assert "csrftoken" in res.cookies, dict(res.cookies)


def test_token_is_usable_for_form_posts(browser, base, user):
    # The primed browser client (used by every form-POST test here) proves
    # the token end to end: without a working token these POSTs would 200
    # on the CSRF failure page instead of 302.
    res = browser().post("/auth/sign-in/", data={"email": user["email"], "password": user["password"]})
    assert res.status_code == 302
    assert res.headers["location"] == base


# ---------------------------------------------------------------------------
# Origin verification (NEWPIDASH-12)
#
# ``CsrfViewMiddleware.process_view`` verifies ``Origin`` before the token
# (``_origin_verified``): an Origin that is neither the request's own
# ``scheme://host`` nor in ``CSRF_TRUSTED_ORIGINS`` is rejected even when
# the cookie and the request token match. The plain auth Views answer the
# ``csrf_failure`` page (200); the DRF auth views run the same
# ``process_view`` through ``SessionAuthentication.enforce_csrf`` and answer
# 403 ``{"detail": "CSRF Failed: Origin checking failed - ..."}``.
# ---------------------------------------------------------------------------

EVIL = "https://evil.example"


def _origin_failed(origin: str) -> bytes:
    return (
        '{"detail":"CSRF Failed: Origin checking failed - '
        + origin
        + ' does not match any trusted origins."}'
    ).encode()


def _session_set(res) -> bool:
    return any(c.startswith(SESSION_COOKIE_NAME + "=") for c in res.headers.get_list("set-cookie"))


def _plain_form(path: str, user: dict) -> dict:
    if "reset-password" in path:
        return {"password": NEW_PASSWORD}
    if "sign-out" in path:
        return {}
    return {"email": user["email"], "password": user["password"]}


def _plain_path(path: str, user: dict) -> str:
    if "{uid}" in path:
        uidb64, token = tokens.mint_reset_token(database_url(), secret_key(), user["id"])
        return path.format(uid=uidb64, token=token)
    return path


PLAIN_PATHS = [
    "/auth/sign-in/",
    "/auth/sign-up/",
    "/auth/spaces/sign-in/",
    "/auth/spaces/sign-up/",
    "/auth/sign-out/",
    "/auth/spaces/sign-out/",
    "/auth/reset-password/{uid}/{token}/",
    "/auth/spaces/reset-password/{uid}/{token}/",
]


@pytest.mark.parametrize("path", PLAIN_PATHS)
def test_plain_view_cross_origin_with_valid_token_hits_failure_page(browser, user, path):
    client = browser()
    # Reference page: the same View's CSRF failure for a token-less POST.
    bare = browser()
    bare.headers.pop("X-CSRFToken")
    reference = bare.post(_plain_path(path, user), data=_plain_form(path, user))
    assert reference.status_code == 200, reference.text[:200]
    assert "templates/csrf_failure.html" in reference.text

    res = client.post(
        _plain_path(path, user),
        data=_plain_form(path, user),
        headers={"Origin": EVIL, "Referer": EVIL + "/"},
    )
    assert res.status_code == 200, (res.status_code, res.headers.get("location"))
    assert res.headers["content-type"] == "text/html; charset=utf-8"
    assert res.content == reference.content
    assert not _session_set(res), res.headers.get_list("set-cookie")


def test_sign_in_cross_origin_does_not_sign_in(browser, user):
    client = browser()
    res = client.post(
        "/auth/sign-in/",
        data={"email": user["email"], "password": user["password"]},
        headers={"Origin": EVIL},
    )
    assert res.status_code == 200
    assert "location" not in res.headers
    assert client.get("/api/users/me/").status_code == 401


def test_sign_in_null_origin_hits_failure_page(browser, user):
    res = browser().post(
        "/auth/sign-in/",
        data={"email": user["email"], "password": user["password"]},
        headers={"Origin": "null"},
    )
    assert res.status_code == 200
    assert "templates/csrf_failure.html" in res.text
    assert not _session_set(res)


def test_sign_in_same_origin_succeeds(browser, base, user):
    res = browser().post(
        "/auth/sign-in/",
        data={"email": user["email"], "password": user["password"]},
        headers={"Origin": base},
    )
    assert res.status_code == 302, res.text[:200]
    assert res.headers["location"] == base
    assert _session_set(res)


DRF_ROUTES = [
    ("/auth/change-password/", {"old_password": PASSWORD, "new_password": NEW_PASSWORD}),
    ("/auth/set-password/", {"password": NEW_PASSWORD}),
    ("/auth/email-check/", {"email": "someone@example.com"}),
    ("/auth/spaces/email-check/", {"email": "someone@example.com"}),
    ("/auth/forgot-password/", {"email": "someone@example.com"}),
    ("/auth/spaces/forgot-password/", {"email": "someone@example.com"}),
]


@pytest.mark.parametrize(("path", "payload"), DRF_ROUTES)
def test_drf_view_cross_origin_with_valid_token_is_403(browser, user, path, payload):
    res = browser(login_cookies(user)).post(path, json=payload, headers={"Origin": EVIL})
    assert res.status_code == 403, res.text[:200]
    assert res.headers["content-type"] == "application/json"
    assert res.content == _origin_failed(EVIL)


def test_change_password_cross_origin_keeps_the_password(browser, base, user):
    browser(login_cookies(user)).post(
        "/auth/change-password/",
        json={"old_password": PASSWORD, "new_password": NEW_PASSWORD},
        headers={"Origin": EVIL},
    )
    res = browser().post("/auth/sign-in/", data={"email": user["email"], "password": PASSWORD})
    assert res.status_code == 302
    assert res.headers["location"] == base


def test_change_password_origin_is_checked_before_the_cookie(base, user):
    # No csrftoken cookie and no token: Django still reports the Origin
    # failure, because ``process_view`` checks Origin first.
    with httpx.Client(base_url=base, timeout=10, follow_redirects=False) as client:
        client.cookies.update(login_cookies(user))
        res = client.post(
            "/auth/change-password/",
            json={"old_password": PASSWORD, "new_password": NEW_PASSWORD},
            headers={"Origin": "null"},
        )
    assert res.status_code == 403, res.text[:200]
    assert res.content == _origin_failed("null")


def test_change_password_same_origin_succeeds(browser, base, user):
    res = browser(login_cookies(user)).post(
        "/auth/change-password/",
        json={"old_password": PASSWORD, "new_password": NEW_PASSWORD},
        headers={"Origin": base},
    )
    assert res.status_code == 200, res.text[:200]
    assert res.json() == {"message": "Password updated successfully"}


def test_anonymous_drf_view_ignores_origin(anon):
    # ``SessionAuthentication`` only enforces CSRF for an authenticated
    # user; anonymous email-check is unaffected by a foreign Origin.
    res = anon.post("/auth/email-check/", json={}, headers={"Origin": EVIL})
    assert res.status_code == 400, res.text[:200]
