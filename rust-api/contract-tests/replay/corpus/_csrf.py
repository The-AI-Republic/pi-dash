# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared CSRF-prime helpers for the browser-flow corpus sessions.

Every CSRF-enforced POST (all plain-Django form Views, plus the
authenticated DRF posts under stock SessionAuthentication) is preceded
by a fresh ``GET /auth/get-csrf-token/`` prime, and the POST carries
that prime's cookie + header pair. Login rotates the secret, so a prime
is never reused across a login; each prime has unique capture names.
"""

from ..session import Request


def prime(tag: str) -> Request:
    """Fresh CSRF cookie + header pair (unique captures per prime)."""
    return Request(
        "GET", "/auth/get-csrf-token/",
        route="/auth/get-csrf-token/",
        auth="anonymous",
        capture={
            f"csrf_t_{tag}": "csrf_token",
            f"csrf_c_{tag}": "@cookie:csrftoken",
        },
        expect_status=200,
    )


def csrf(tag: str) -> dict:
    """Headers + cookies carrying prime ``tag``'s pair (splat into Request)."""
    return {
        "headers": {"X-CSRFToken": "{csrf_t_%s}" % tag},
        "cookies": {"csrftoken": "{csrf_c_%s}" % tag},
    }
