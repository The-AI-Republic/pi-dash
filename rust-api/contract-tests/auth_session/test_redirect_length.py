# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Over-long sign-in error redirects keep ``next_path`` (PIDASHCONV-1354).

Django 4.2.30's ``url_has_allowed_host_and_scheme`` has no length cap
(the 2048 limit is a Django 6 rule), so ``get_safe_redirect_url`` keeps
``next_path`` on redirects up to 16384 characters; past that
``HttpResponseRedirectBase`` raises ``DisallowedRedirect`` (400).
"""

from _harness.redirects import location_of, query_of


def _long_invalid_email(local_len: int) -> str:
    # No domain: fails validate_email -> INVALID_EMAIL_SIGN_IN (5075),
    # echoing the address in the error params.
    return "a" * local_len + "@"


def test_error_redirect_between_2049_and_16384_chars_keeps_next_path(browser, base):
    res = browser().post(
        "/auth/sign-in/",
        data={
            "email": _long_invalid_email(2000),
            "password": "x",
            "next_path": "/ws/projects/",
        },
    )
    location = location_of(res)
    # Guard the premise: the Location must actually exercise the
    # (2048, 16384] window, or the assertion below is vacuous.
    assert 2048 < len(location) <= 16384, len(location)
    params = query_of(location)
    assert params["next_path"] == "/ws/projects/"
    assert params["error_code"] == "5075"


def test_error_redirect_over_16384_chars_answers_400(browser):
    res = browser().post(
        "/auth/sign-in/",
        data={
            "email": _long_invalid_email(17000),
            "password": "x",
            "next_path": "/ws/projects/",
        },
    )
    # Status only: the DEBUG oracle renders the technical
    # DisallowedRedirect page, which no port can reproduce byte for byte.
    assert res.status_code == 400, (res.status_code, res.text[:200])
