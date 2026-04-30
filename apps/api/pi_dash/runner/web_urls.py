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
    MachineTokenListCreateEndpoint,
    MachineTokenRevokeEndpoint,
    PodDetailEndpoint,
    PodListEndpoint,
    RegistrationTokenCreateEndpoint,
    RunnerDetailEndpoint,
    RunnerListEndpoint,
    RunnerRevokeEndpoint,
)

urlpatterns = [
    path("", RunnerListEndpoint.as_view(), name="runner-list"),
    path(
        "tokens/",
        RegistrationTokenCreateEndpoint.as_view(),
        name="runner-tokens",
    ),
    # Machine tokens (multi-runner connections). See design.md §5.3.
    path(
        "machine-tokens/",
        MachineTokenListCreateEndpoint.as_view(),
        name="machine-token-list",
    ),
    path(
        "machine-tokens/<uuid:token_id>/revoke/",
        MachineTokenRevokeEndpoint.as_view(),
        name="machine-token-revoke",
    ),
    path(
        "<uuid:runner_id>/",
        RunnerDetailEndpoint.as_view(),
        name="runner-detail",
    ),
    path(
        "<uuid:runner_id>/revoke/",
        RunnerRevokeEndpoint.as_view(),
        name="runner-revoke",
    ),
    # Pods
    path("pods/", PodListEndpoint.as_view(), name="pod-list"),
    path(
        "pods/<uuid:pod_id>/",
        PodDetailEndpoint.as_view(),
        name="pod-detail",
    ),
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
