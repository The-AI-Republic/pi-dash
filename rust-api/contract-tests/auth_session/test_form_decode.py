# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Raw urlencoded edge cases on the password endpoints (PIDASHCONV-1076).

Django's ``QueryDict`` never raises on malformed percent escapes: invalid
``%`` sequences stay literal and the view proceeds. The Rust port used to
hand-roll this decoder and sliced ``&str`` at byte indices, so a ``%``
immediately followed by a 3- or 4-byte UTF-8 char panicked the handler
(``byte index is not a char boundary``) on every POST endpoint in
``password_routes`` — including the unauthenticated forgot/reset flows.

Bodies are sent as raw bytes (``content=`` + explicit content-type):
``httpx`` ``data=`` would percent-encode the payload and hide the bug.
Every case asserts the exact status + body/location on whichever backend
``BASE_URL`` points at, so the suite proves byte-identical behaviour when
run against Django and against Rust.
"""

import pytest

from _harness.redirects import assert_error_redirect, location_of
from _harness.settings import database_url, secret_key

from . import tokens
from .conftest import NEW_PASSWORD
from .test_reset_password import SUCCESS_QUERY

FORM = {"Content-Type": "application/x-www-form-urlencoded"}

# `%` followed by a 2-, 3- and 4-byte UTF-8 char (é U+00E9, ☃ U+2603,
# 😀 U+1F600), plus the trailing-`%` and short/invalid-escape edges.
# Written as explicit byte escapes so no editor or transport re-encodes them.
WEIRD_VALUES = [
    b"%\xc3\xa9",  # % + 2-byte é
    b"%\xe2\x98\x83",  # % + 3-byte ☃ (panicked pre-fix)
    b"%\xf0\x9f\x98\x80",  # % + 4-byte 😀 (panicked pre-fix)
    b"%",  # trailing %
    b"%A",  # short escape
    b"%zz",  # invalid hex
]

INVALID_EMAIL = {"error_code": 5005, "error_message": "INVALID_EMAIL"}


@pytest.mark.parametrize("weird", WEIRD_VALUES, ids=["2byte", "3byte", "4byte", "trailing", "short", "badhex"])
def test_forgot_password_percent_before_multibyte(anon, weird):
    # No "@" anywhere, so validate_email fails identically on both
    # backends and the view answers INVALID_EMAIL — never a 500.
    res = anon.post("/auth/forgot-password/", content=b"email=" + weird, headers=FORM)
    assert res.status_code == 400, res.text[:300]
    assert res.json() == INVALID_EMAIL


@pytest.mark.parametrize("weird", WEIRD_VALUES, ids=["2byte", "3byte", "4byte", "trailing", "short", "badhex"])
def test_space_forgot_password_percent_before_multibyte(anon, weird):
    res = anon.post(
        "/auth/spaces/forgot-password/", content=b"email=" + weird, headers=FORM
    )
    assert res.status_code == 400, res.text[:300]
    assert res.json() == INVALID_EMAIL


def _fresh_token(user):
    return tokens.mint_reset_token(database_url(), secret_key(), user["id"])


@pytest.mark.parametrize("weird", WEIRD_VALUES, ids=["2byte", "3byte", "4byte", "trailing", "short", "badhex"])
def test_reset_password_ignores_weird_extra_field(browser, base, user, weird):
    # The junk field exercises the form parser (it panicked pre-fix) while
    # a valid password keeps the verdict a success redirect on both backends.
    uidb64, token = _fresh_token(user)
    body = b"password=" + NEW_PASSWORD.encode() + b"&junk=" + weird
    res = browser().post(
        f"/auth/reset-password/{uidb64}/{token}/", content=body, headers=FORM
    )
    assert res.status_code == 302, res.text[:300]
    assert location_of(res) == f"{base}/{SUCCESS_QUERY}"


def test_reset_password_weird_password_is_weak(browser, base, user):
    # A two-codepoint password is present but weak on both backends.
    uidb64, token = _fresh_token(user)
    body = b"password=%\xe2\x98\x83"
    res = browser().post(
        f"/auth/reset-password/{uidb64}/{token}/", content=body, headers=FORM
    )
    assert_error_redirect(res, base=base, error_code=5021)


def test_space_reset_password_ignores_weird_extra_field(browser, space_base, user):
    uidb64, token = _fresh_token(user)
    body = b"password=" + NEW_PASSWORD.encode() + b"&junk=%\xe2\x98\x83"
    res = browser().post(
        f"/auth/spaces/reset-password/{uidb64}/{token}/", content=body, headers=FORM
    )
    assert res.status_code == 302, res.text[:300]
    assert location_of(res) == space_base + "/"
