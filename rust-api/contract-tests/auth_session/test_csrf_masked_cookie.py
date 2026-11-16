# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""64-char `csrftoken` cookies unmask before comparison (PIDASHCONV-1311).

Django's `CsrfViewMiddleware._get_secret` unmasks a 64-char cookie
(pre-4.0 clients stored the masked token) before `_does_token_match`
(`django/middleware/csrf.py`), so a form POST carrying a masked cookie
plus any valid token for that secret proceeds to the view. `POST
/auth/sign-in/` with an unknown email must therefore take the
USER_DOES_NOT_EXIST error redirect, never the CSRF failure page.
"""

import secrets
import urllib.parse

import httpx

from _harness.redirects import assert_error_redirect

from .conftest import _unique_email

_CSRF_ALLOWED_CHARS = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"


def _mask_secret(secret: str) -> str:
    """Mirror Django `_mask_cipher_token`: fresh 32-char mask + cipher."""
    mask = "".join(secrets.choice(_CSRF_ALLOWED_CHARS) for _ in range(32))
    cipher = "".join(
        _CSRF_ALLOWED_CHARS[(_CSRF_ALLOWED_CHARS.index(s) + _CSRF_ALLOWED_CHARS.index(m)) % 62]
        for s, m in zip(secret, mask)
    )
    return mask + cipher


def _post_sign_in(base, cookie, token):
    body = urllib.parse.urlencode({"email": _unique_email(), "password": "x"})
    return httpx.post(
        f"{base}/auth/sign-in/",
        content=body,
        headers={
            "Cookie": f"csrftoken={cookie}",
            "X-CSRFToken": token,
            "Referer": f"{base}/",
            "Content-Type": "application/x-www-form-urlencoded",
        },
        follow_redirects=False,
        timeout=10,
    )


def test_masked_cookie_with_matching_token_proceeds(base):
    primed = httpx.Client(base_url=base, timeout=10, follow_redirects=False)
    try:
        primed.get("/auth/get-csrf-token/").raise_for_status()
        secret = primed.cookies.get("csrftoken")
    finally:
        primed.close()
    assert secret and len(secret) == 32, secret

    cookie = _mask_secret(secret)
    assert len(cookie) == 64

    # A token masked under a *different* mask still unmasks to the secret.
    res = _post_sign_in(base, cookie, _mask_secret(secret))
    assert "CSRF Verification Failed" not in res.text
    assert_error_redirect(res, base=base, error_code=5060)

    # The cookie value itself is likewise a valid token for that cookie.
    res = _post_sign_in(base, cookie, cookie)
    assert "CSRF Verification Failed" not in res.text
    assert_error_redirect(res, base=base, error_code=5060)
