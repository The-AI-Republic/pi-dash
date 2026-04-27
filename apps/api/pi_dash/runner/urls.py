# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""External runner-facing API (daemon traffic) — mounted at ``/api/v1/runner/``.

All routes here use the runner's bearer credential for auth except
``register/`` and ``health/`` which are public (they bootstrap the credential
itself).
"""

from django.urls import path

from pi_dash.runner.views import (
    HealthEndpoint,
    MetricsEndpoint,
    RegisterEndpoint,
    RunnerDeregisterEndpoint,
    RunnerRotateEndpoint,
    TokenRunnerCreateEndpoint,
)

app_name = "runner"

urlpatterns = [
    path("health/", HealthEndpoint.as_view(), name="health"),
    path("metrics/", MetricsEndpoint.as_view(), name="metrics"),
    path("register/", RegisterEndpoint.as_view(), name="register"),
    # Token-authenticated runner registration. Used by `pidash configure
    # runner --name <NAME>` to add an additional runner under an
    # existing token without re-running the one-time enrolment flow.
    path(
        "register-under-token/",
        TokenRunnerCreateEndpoint.as_view(),
        name="register-under-token",
    ),
    path(
        "<uuid:runner_id>/deregister/",
        RunnerDeregisterEndpoint.as_view(),
        name="deregister",
    ),
    path(
        "<uuid:runner_id>/rotate/",
        RunnerRotateEndpoint.as_view(),
        name="rotate",
    ),
]
