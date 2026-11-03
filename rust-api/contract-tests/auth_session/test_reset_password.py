# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""POST /auth/reset-password/<uidb64>/<token>/ (+ space twin) — Views, 302.

Tokens are minted with the stdlib replica of Django 4.2's
``PasswordResetTokenGenerator`` (see ``tokens.py``). Pinned quirks:

- app success lands on ``<base>/sign-in?success=True``; space success on
  the space base itself
- app answers INVALID_PASSWORD_TOKEN (5125) for an unknown UUID user;
  space lets ``DoesNotExist`` escape → HTTP 500
- a uid that is not a UUID at all → HTTP 500 on both (field
  ``ValidationError`` escapes both Views)
- non-UTF-8 uid bytes: app → 5125 (the inner ``except (ValueError, ...)``
  swallows the decode error, so the EXPIRED branch is dead there);
  space → EXPIRED_PASSWORD_TOKEN (5130) at a double-slash URL
"""

from _harness.redirects import assert_error_redirect, location_of
from _harness.settings import database_url, secret_key

from . import tokens
from .conftest import NEW_PASSWORD, WEAK_PASSWORD

SUCCESS_QUERY = "sign-in?success=True"


def _fresh_token(user):
    return tokens.mint_reset_token(database_url(), secret_key(), user["id"])


def test_success_redirects_to_sign_in(browser, base, user):
    uidb64, token = _fresh_token(user)
    res = browser().post(f"/auth/reset-password/{uidb64}/{token}/", data={"password": NEW_PASSWORD})
    assert res.status_code == 302
    assert location_of(res) == f"{base}/{SUCCESS_QUERY}"


def test_new_password_signs_in(browser, base, user):
    uidb64, token = _fresh_token(user)
    browser().post(f"/auth/reset-password/{uidb64}/{token}/", data={"password": NEW_PASSWORD})
    res = browser().post("/auth/sign-in/", data={"email": user["email"], "password": NEW_PASSWORD})
    assert res.status_code == 302
    assert location_of(res) == base


def test_token_is_single_use(browser, base, user):
    uidb64, token = _fresh_token(user)
    browser().post(f"/auth/reset-password/{uidb64}/{token}/", data={"password": NEW_PASSWORD})
    res = browser().post(f"/auth/reset-password/{uidb64}/{token}/", data={"password": NEW_PASSWORD})
    assert_error_redirect(res, base=base, error_code=5125)


def test_weak_password(browser, base, user):
    uidb64, token = _fresh_token(user)
    res = browser().post(f"/auth/reset-password/{uidb64}/{token}/", data={"password": WEAK_PASSWORD})
    assert_error_redirect(res, base=base, error_code=5021)


def test_missing_password(browser, base, user):
    uidb64, token = _fresh_token(user)
    res = browser().post(f"/auth/reset-password/{uidb64}/{token}/", data={})
    assert_error_redirect(res, base=base, error_code=5020)


def test_garbage_token(browser, base, user):
    uidb64, _ = _fresh_token(user)
    res = browser().post(
        f"/auth/reset-password/{uidb64}/bogus-token/", data={"password": NEW_PASSWORD}
    )
    assert_error_redirect(res, base=base, error_code=5125)


def test_unknown_uuid_user(browser, base):
    res = browser().post(
        f"/auth/reset-password/{tokens.ghost_uidb64()}/bogus-token/",
        data={"password": NEW_PASSWORD},
    )
    assert_error_redirect(res, base=base, error_code=5125)


def test_non_uuid_uid_returns_500(browser, user):
    uidb64, token = _fresh_token(user)
    res = browser().post(
        f"/auth/reset-password/{tokens.non_uuid_uidb64()}/{token}/",
        data={"password": NEW_PASSWORD},
    )
    assert res.status_code == 500


def test_space_success_redirects_to_space_base(browser, space_base, user):
    uidb64, token = _fresh_token(user)
    res = browser().post(
        f"/auth/spaces/reset-password/{uidb64}/{token}/", data={"password": NEW_PASSWORD}
    )
    assert res.status_code == 302
    assert location_of(res) == space_base + "/"


def test_space_new_password_signs_in(browser, space_base, user):
    uidb64, token = _fresh_token(user)
    browser().post(f"/auth/spaces/reset-password/{uidb64}/{token}/", data={"password": NEW_PASSWORD})
    res = browser().post(
        "/auth/spaces/sign-in/", data={"email": user["email"], "password": NEW_PASSWORD}
    )
    assert res.status_code == 302
    assert location_of(res) == space_base


def test_space_weak_password(browser, space_base, user):
    uidb64, token = _fresh_token(user)
    res = browser().post(
        f"/auth/spaces/reset-password/{uidb64}/{token}/", data={"password": WEAK_PASSWORD}
    )
    assert_error_redirect(res, base=space_base, error_code=5021)


def test_space_unknown_uuid_user_returns_500(browser, user):
    uidb64, token = _fresh_token(user)
    res = browser().post(
        f"/auth/spaces/reset-password/{tokens.ghost_uidb64()}/{token}/",
        data={"password": NEW_PASSWORD},
    )
    assert res.status_code == 500


def test_space_non_utf8_uid_is_expired_token(browser, space_base, user):
    _, token = _fresh_token(user)
    res = browser().post(
        f"/auth/spaces/reset-password/{tokens.bad_utf8_uidb64()}/{token}/",
        data={"password": NEW_PASSWORD},
    )
    # The space base already ends in "/": the error URL keeps a double slash.
    assert res.status_code == 302
    assert location_of(res).startswith(space_base + "//accounts/reset-password/")
    assert_error_redirect(res, base=space_base, error_code=5130)


def test_app_non_utf8_uid_is_invalid_token(browser, base, user):
    _, token = _fresh_token(user)
    res = browser().post(
        f"/auth/reset-password/{tokens.bad_utf8_uidb64()}/{token}/",
        data={"password": NEW_PASSWORD},
    )
    assert_error_redirect(res, base=base, error_code=5125)
