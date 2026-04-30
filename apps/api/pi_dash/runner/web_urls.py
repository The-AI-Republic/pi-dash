# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Web-app facing runner API (session auth) — mounted under ``/api/runners/``."""

from django.urls import path

from pi_dash.runner.views import (
    AgentRunCancelEndpoint,
    AgentRunDetailEndpoint,
    AgentRunListEndpoint,
    AgentRunReleasePinEndpoint,
    ApprovalDecideEndpoint,
    ApprovalListEndpoint,
    ConnectionDetailEndpoint,
    ConnectionListCreateEndpoint,
    PodDetailEndpoint,
    PodListEndpoint,
    ProjectListEndpoint,
    RunnerDetailEndpoint,
    RunnerListEndpoint,
)

urlpatterns = [
    path("", RunnerListEndpoint.as_view(), name="runner-list"),
    # Connections (paired dev machines).
    path(
        "connections/",
        ConnectionListCreateEndpoint.as_view(),
        name="connection-list",
    ),
    path(
        "connections/<uuid:connection_id>/",
        ConnectionDetailEndpoint.as_view(),
        name="connection-detail",
    ),
    path(
        "<uuid:runner_id>/",
        RunnerDetailEndpoint.as_view(),
        name="runner-detail",
    ),
    # Pods
    path("pods/", PodListEndpoint.as_view(), name="pod-list"),
    path(
        "pods/<uuid:pod_id>/",
        PodDetailEndpoint.as_view(),
        name="pod-detail",
    ),
    # Projects (read-only, scoped to caller's workspaces).
    path("projects/", ProjectListEndpoint.as_view(), name="project-list"),
    # Runs
    path("runs/", AgentRunListEndpoint.as_view(), name="runner-runs"),
    path(
        "runs/<uuid:run_id>/",
        AgentRunDetailEndpoint.as_view(),
        name="runner-run-detail",
    ),
    path(
        "runs/<uuid:run_id>/cancel/",
        AgentRunCancelEndpoint.as_view(),
        name="runner-run-cancel",
    ),
    path(
        "runs/<uuid:run_id>/release-pin/",
        AgentRunReleasePinEndpoint.as_view(),
        name="runner-run-release-pin",
    ),
    path(
        "approvals/",
        ApprovalListEndpoint.as_view(),
        name="runner-approvals",
    ),
    path(
        "approvals/<uuid:approval_id>/decide/",
        ApprovalDecideEndpoint.as_view(),
        name="runner-approval-decide",
    ),
]
