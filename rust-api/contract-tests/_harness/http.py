# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""httpx client factories. Auth is the admin session cookie the server sets
for /instances paths (ADMIN_SESSION_COOKIE_NAME = "admin-session-id")."""

import httpx

from . import signing
from .settings import base_url


def anon_client(**kwargs):
    return httpx.Client(base_url=base_url(), timeout=15, **kwargs)


def admin_client(session_key, **kwargs):
    return httpx.Client(
        base_url=base_url(),
        cookies={signing.ADMIN_SESSION_COOKIE: session_key},
        timeout=15,
        **kwargs,
    )
