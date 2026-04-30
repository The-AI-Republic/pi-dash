# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Public health endpoint for the runner-facing API.

The full enrollment flow now lives in
:mod:`pi_dash.runner.views.connections`. This module is kept tiny on
purpose: only ``GET /api/v1/runner/health/`` lives here, since it must
remain accessible without auth so a daemon can probe the cloud during
``pi-dash-runner doctor``.
"""

from rest_framework.permissions import AllowAny
from rest_framework.response import Response
from rest_framework.views import APIView


HEARTBEAT_INTERVAL_SECS = 25
PROTOCOL_VERSION = 3


class HealthEndpoint(APIView):
    authentication_classes: list = []
    permission_classes = [AllowAny]

    def get(self, request):
        return Response({"ok": True, "protocol_version": PROTOCOL_VERSION})
