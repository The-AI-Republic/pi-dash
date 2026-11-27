# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Global request-body size limit (``RequestBodySizeLimitMiddleware``).

Django rejects any request whose body exceeds ``DATA_UPLOAD_MAX_MEMORY_SIZE``
(``int(get_config("FILE_SIZE_LIMIT", 5242880))``, ``settings/common.py:594``)
with a 413 ``JsonResponse`` — before URL resolution, auth, CSRF, and throttling
(the middleware reads ``request.body`` in its request phase). The Rust server
must answer the identical status, content-type, and bytes on every domain
family (``BodyLimitLayer`` in the serve-wide stack, sole enforcer).

DB-free and auth-free: over-limit bodies never reach a view on either backend,
so anonymous probes to real POST routes pin the middleware exactly. The suite
runs unchanged against Django and against Rust (``BASE_URL`` selects the
backend); byte-identical output on both runs is the parity proof.

Boundary shape (``<=`` passes, ``limit + 1`` rejects) is pinned from above by
every over-limit probe (exactly ``LIMIT + 1`` bytes) and from below by the
at-limit probe on a Rust-owned ``Bytes`` route (``POST /api/workspaces/``):
without ``DefaultBodyLimit::disable()`` in the stack, axum's own 2MB extractor
default answers 413 with ``Failed to buffer the request body`` for the 2-5MB
window — the failure this issue fixes.
"""

import httpx
import pytest

from _harness.client import base_url, client  # noqa: F401  (fixtures)

pytestmark = pytest.mark.contract

LIMIT = 5_242_880
OVER = LIMIT + 1
EXPECTED_413 = (
    b'{"error": "REQUEST_BODY_TOO_LARGE", '
    b'"detail": "The size of the request body exceeds the maximum allowed size."}'
)

# One real POST route per RouteGroup family (``overlay.rs``). Auth, method
# allow-listing, and slug validity are irrelevant: the size check runs before
# all of them on both backends.
FAMILY_ROUTES = [
    ("web", "/"),
    ("app", "/api/workspaces/"),
    ("assistant", "/api/workspaces/x/ai-assistant/threads/"),
    ("loop", "/api/users/me/auto-pm/"),
    ("prompting", "/api/workspaces/x/prompt-sections"),
    (
        "space",
        "/api/public/anchor/x/intakes/00000000-0000-0000-0000-000000000000/intake-issues/",
    ),
    ("license", "/api/instances/admins/sign-up-screen-visited/"),
    ("runner-web", "/api/runners/"),
    ("api-v1", "/api/v1/assets/user-assets/"),
    ("runner", "/api/v1/runner/health/"),
    ("auth", "/auth/sign-in/"),
]


@pytest.mark.parametrize(("family", "path"), FAMILY_ROUTES)
def test_over_limit_body_rejected_with_identical_bytes(
    client: httpx.Client, family: str, path: str
):
    resp = client.post(
        path,
        content=b"x" * OVER,
        headers={"Content-Type": "application/json"},
    )
    assert resp.status_code == 413, (family, resp.status_code, resp.content[:100])
    assert resp.headers["content-type"] == "application/json", family
    assert resp.content == EXPECTED_413, family


def test_at_limit_body_passes_the_layer(client: httpx.Client):
    # Exactly LIMIT bytes of valid JSON: both backends must run the view
    # instead of 413 — anonymous workspace creation answers the same 401
    # denial bytes on both. Pre-fix Rust 413s here on owned Bytes routes
    # (axum's 2MB extractor default).
    pad = b"x" * (LIMIT - len(b'{"pad": ""}'))
    resp = client.post(
        "/api/workspaces/",
        content=b'{"pad": "' + pad + b'"}',
        headers={"Content-Type": "application/json"},
    )
    assert resp.status_code == 401, (resp.status_code, resp.content[:100])
    assert resp.content == b'{"detail":"Authentication credentials were not provided."}'
