# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""CE seam for "did this request come from the Pi Dash desktop app?".

Three endpoints are desktop-only — machine enrollment, the agent model profile,
and the agent model token. Each hands the caller something a browser tab has no
business holding, so "is authenticated" is not a strong enough gate.

CE marks desktop sessions with a session key, because the open-source build
authenticates with Django sessions. The cloud overlay replaces this module to
read the ``client`` claim its OIDC exchange stamps on the desktop access token,
which is the stronger check (a claim cannot be set by a later request).
"""

from __future__ import annotations

#: Session key CE uses to remember that this session was opened by the desktop
#: app. Set at sign-in by the desktop flow; absent for browser sessions.
DESKTOP_SESSION_KEY = "pidash_client"
DESKTOP_CLIENT = "desktop"


def request_is_desktop(request) -> bool:
    """True when ``request`` belongs to a Pi Dash desktop session."""
    session = getattr(request, "session", None)
    if session is None:
        return False
    try:
        return session.get(DESKTOP_SESSION_KEY) == DESKTOP_CLIENT
    except Exception:  # noqa: BLE001 — a broken session store is not a desktop
        return False
