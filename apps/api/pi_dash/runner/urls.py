# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""External runner-facing API (daemon traffic) — mounted at ``/api/v1/runner/``.

Health is public; everything else uses ConnectionBearerAuthentication.
The connection-enroll path lives outside the runner namespace at
``/api/v1/connections/enroll/`` (see project urls).
"""

from django.urls import path

from pi_dash.runner.views import (
    ConnectionEnrollEndpoint,
    ConnectionRunnerDeleteEndpoint,
    ConnectionRunnerListCreateEndpoint,
    HealthEndpoint,
    MetricsEndpoint,
)

app_name = "runner"

urlpatterns = [
    path("health/", HealthEndpoint.as_view(), name="health"),
    path("metrics/", MetricsEndpoint.as_view(), name="metrics"),
    # Connection enrollment + per-connection runner CRUD. These live under
    # the runner namespace so they share the same /api/v1/runner/ prefix
    # the daemon already uses.
    path(
        "connections/enroll/",
        ConnectionEnrollEndpoint.as_view(),
        name="connection-enroll",
    ),
    path(
        "connections/<uuid:connection_id>/runners/",
        ConnectionRunnerListCreateEndpoint.as_view(),
        name="connection-runners",
    ),
    path(
        "connections/<uuid:connection_id>/runners/<uuid:runner_id>/",
        ConnectionRunnerDeleteEndpoint.as_view(),
        name="connection-runner-detail",
    ),
]
