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
    MachineTokenTicketEndpoint,
    PodDetailEndpoint,
    PodListEndpoint,
    ProjectListEndpoint,
    RunnerDetailEndpoint,
    RunnerInviteEndpoint,
    RunnerListEndpoint,
)

urlpatterns = [
    path("", RunnerListEndpoint.as_view(), name="runner-list"),
    # Mint a runner-enrollment invite (one-time token). Replaces the
    # legacy ``connections/`` create flow.
    path(
        "invites/",
        RunnerInviteEndpoint.as_view(),
        name="runner-invite",
    ),
    path(
        "machine-tokens/<uuid:workspace_id>/tickets/",
        MachineTokenTicketEndpoint.as_view(),
        name="machine-token-ticket",
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
