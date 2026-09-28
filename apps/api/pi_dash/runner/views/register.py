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

from importlib import metadata

from django.conf import settings
from rest_framework.permissions import AllowAny
from rest_framework.response import Response
from rest_framework.views import APIView


def _api_version() -> str:
    """Best-effort deployed-package version for deploy-lag diagnosis.

    A hung runner incident (wedged session-opens fixed by #246 but still
    reproducing in production) came down to a stale deployment that was
    indistinguishable from current code from the outside — this endpoint
    reported a hardcoded protocol version while the session-open path
    enforced a newer one. Report the real enforced version plus the
    installed package version so ``curl .../health/`` dates a deployment.
    """
    try:
        return metadata.version("pi-dash")
    except metadata.PackageNotFoundError:
        return "unknown"


class HealthEndpoint(APIView):
    authentication_classes: list = []
    permission_classes = [AllowAny]

    def get(self, request):
        return Response(
            {
                "ok": True,
                "protocol_version": settings.RUNNER_PROTOCOL_VERSION,
                "api_version": _api_version(),
            }
        )
