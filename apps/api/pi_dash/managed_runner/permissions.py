# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Desktop-only permission for the managed runner's provisioning endpoints."""

from __future__ import annotations

from rest_framework.permissions import BasePermission


class IsDesktopSession(BasePermission):
    """Allow only requests from an authenticated Pi Dash desktop session.

    A web session is refused with ``desktop_session_required`` rather than a
    bare 403 so the client can tell "you are not signed in" apart from "this
    endpoint is not for browsers".
    """

    message = {"error": "desktop_session_required", "detail": "This endpoint is available to the Pi Dash desktop app."}

    def has_permission(self, request, view) -> bool:
        user = getattr(request, "user", None)
        if user is None or not user.is_authenticated:
            return False
        from pi_dash.ee.authentication.desktop import request_is_desktop

        return request_is_desktop(request)
