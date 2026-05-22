# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Web-app facing runner API (session auth) — mounted under ``/api/runners/``."""

from django.urls import path

from pi_dash.runner.views import (
    AgentChatApprovalDecideEndpoint,
    AgentChatApprovalListEndpoint,
    AgentChatCancelEndpoint,
    AgentChatCloseEndpoint,
    AgentChatMessageListEndpoint,
    AgentChatSessionDetailEndpoint,
    AgentChatSessionListEndpoint,
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
    RunnerReviveEndpoint,
    RunnerRevokeEndpoint,
    chat_event_stream,
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
    path(
        "<uuid:runner_id>/revoke/",
        RunnerRevokeEndpoint.as_view(),
        name="runner-revoke",
    ),
    path(
        "<uuid:runner_id>/revive/",
        RunnerReviveEndpoint.as_view(),
        name="runner-revive",
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
    # Direct runner chat
    path(
        "chat/sessions/",
        AgentChatSessionListEndpoint.as_view(),
        name="runner-chat-sessions",
    ),
    path(
        "chat/sessions/<uuid:session_id>/",
        AgentChatSessionDetailEndpoint.as_view(),
        name="runner-chat-session-detail",
    ),
    path(
        "chat/sessions/<uuid:session_id>/messages/",
        AgentChatMessageListEndpoint.as_view(),
        name="runner-chat-messages",
    ),
    path(
        "chat/sessions/<uuid:session_id>/events/",
        chat_event_stream,
        name="runner-chat-events",
    ),
    path(
        "chat/sessions/<uuid:session_id>/cancel/",
        AgentChatCancelEndpoint.as_view(),
        name="runner-chat-cancel",
    ),
    path(
        "chat/sessions/<uuid:session_id>/close/",
        AgentChatCloseEndpoint.as_view(),
        name="runner-chat-close",
    ),
    path(
        "chat/approvals/",
        AgentChatApprovalListEndpoint.as_view(),
        name="runner-chat-approvals",
    ),
    path(
        "chat/approvals/<uuid:approval_id>/decide/",
        AgentChatApprovalDecideEndpoint.as_view(),
        name="runner-chat-approval-decide",
    ),
]
