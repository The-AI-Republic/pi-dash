# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""External runner-facing API (daemon traffic) — mounted at ``/api/v1/runner/``.

See ``.ai_design/move_to_https/design.md`` §5 / §7. The daemon
authenticates with a shared dev-machine MachineToken, identifies the
speaking runner by URL/header, opens one session per runner, and long-polls
that session for control-plane messages.
"""

from django.urls import path

from pi_dash.runner.views import (
    ChatApprovalEndpoint,
    ChatClosedEndpoint,
    ChatEventEndpoint,
    ChatFailedEndpoint,
    ChatMessageCompleteEndpoint,
    ChatMessageStartedEndpoint,
    ChatStartedEndpoint,
    HealthEndpoint,
    MachineTokenRedeemEndpoint,
    MetricsEndpoint,
    ProjectListEndpoint,
    RunnerCreateEndpoint,
    RunAcceptEndpoint,
    RunApprovalEndpoint,
    RunAwaitingReauthEndpoint,
    RunCancelledEndpoint,
    RunCompletedEndpoint,
    RunEventEndpoint,
    RunFailedEndpoint,
    RunPausedEndpoint,
    RunResumedEndpoint,
    RunStartedEndpoint,
    RunStreamUpgradeEndpoint,
    RunnerEnrollEndpoint,
    RunnerRefreshEndpoint,
    RunnerSelfRevokeEndpoint,
    RunnerSessionDeleteEndpoint,
    RunnerSessionOpenEndpoint,
    runner_session_poll,
)

app_name = "runner"

urlpatterns = [
    path("health/", HealthEndpoint.as_view(), name="health"),
    path("metrics/", MetricsEndpoint.as_view(), name="metrics"),
    # Enrollment + refresh.
    path(
        "runners/enroll/",
        RunnerEnrollEndpoint.as_view(),
        name="runner-enroll",
    ),
    # CLI-initiated runner creation (X-Api-Key auth). Dual of
    # invite+enroll for callers that already have a user-scoped token.
    path(
        "runners/",
        RunnerCreateEndpoint.as_view(),
        name="runner-create",
    ),
    # CLI-facing project list — same view as /api/runners/projects/ but
    # mounted here so `pidash` calls stay under /api/v1/runner/. Auth
    # accepts X-Api-Key (set by `pidash auth login`).
    path(
        "projects/",
        ProjectListEndpoint.as_view(),
        name="cli-project-list",
    ),
    path(
        "runners/<uuid:runner_id>/refresh/",
        RunnerRefreshEndpoint.as_view(),
        name="runner-refresh",
    ),
    # Runner self-deletion (machine-token auth). Symmetric to the web
    # UI's session-auth `/api/runners/<rid>/` DELETE so `pidash runner
    # remove` and the TUI's remove modal can teardown without forcing
    # the operator into the cloud UI.
    path(
        "runners/<uuid:runner_id>/",
        RunnerSelfRevokeEndpoint.as_view(),
        name="runner-self-revoke",
    ),
    # Session lifecycle + poll.
    path(
        "runners/<uuid:runner_id>/sessions/",
        RunnerSessionOpenEndpoint.as_view(),
        name="runner-session-open",
    ),
    path(
        "runners/<uuid:runner_id>/sessions/<uuid:sid>/",
        RunnerSessionDeleteEndpoint.as_view(),
        name="runner-session-delete",
    ),
    path(
        "runners/<uuid:runner_id>/sessions/<uuid:sid>/poll",
        runner_session_poll,
        name="runner-session-poll",
    ),
    # Run-lifecycle + event upstream.
    path(
        "runs/<uuid:run_id>/accept/",
        RunAcceptEndpoint.as_view(),
        name="run-accept",
    ),
    path(
        "runs/<uuid:run_id>/started/",
        RunStartedEndpoint.as_view(),
        name="run-started",
    ),
    path(
        "runs/<uuid:run_id>/events/",
        RunEventEndpoint.as_view(),
        name="run-events",
    ),
    path(
        "runs/<uuid:run_id>/approvals/",
        RunApprovalEndpoint.as_view(),
        name="run-approvals",
    ),
    path(
        "runs/<uuid:run_id>/awaiting-reauth/",
        RunAwaitingReauthEndpoint.as_view(),
        name="run-awaiting-reauth",
    ),
    path(
        "runs/<uuid:run_id>/complete/",
        RunCompletedEndpoint.as_view(),
        name="run-complete",
    ),
    path(
        "runs/<uuid:run_id>/pause/",
        RunPausedEndpoint.as_view(),
        name="run-pause",
    ),
    path(
        "runs/<uuid:run_id>/fail/",
        RunFailedEndpoint.as_view(),
        name="run-fail",
    ),
    path(
        "runs/<uuid:run_id>/cancelled/",
        RunCancelledEndpoint.as_view(),
        name="run-cancelled",
    ),
    path(
        "runs/<uuid:run_id>/resumed/",
        RunResumedEndpoint.as_view(),
        name="run-resumed",
    ),
    path(
        "runs/<uuid:run_id>/stream/upgrade/",
        RunStreamUpgradeEndpoint.as_view(),
        name="run-stream-upgrade",
    ),
    # Direct chat lifecycle + event upstream.
    path(
        "chat/sessions/<uuid:session_id>/started/",
        ChatStartedEndpoint.as_view(),
        name="chat-started",
    ),
    path(
        "chat/sessions/<uuid:session_id>/events/",
        ChatEventEndpoint.as_view(),
        name="chat-events",
    ),
    path(
        "chat/sessions/<uuid:session_id>/messages/<uuid:message_id>/started/",
        ChatMessageStartedEndpoint.as_view(),
        name="chat-message-started",
    ),
    path(
        "chat/sessions/<uuid:session_id>/messages/<uuid:message_id>/complete/",
        ChatMessageCompleteEndpoint.as_view(),
        name="chat-message-complete",
    ),
    path(
        "chat/sessions/<uuid:session_id>/approvals/",
        ChatApprovalEndpoint.as_view(),
        name="chat-approvals",
    ),
    path(
        "chat/sessions/<uuid:session_id>/failed/",
        ChatFailedEndpoint.as_view(),
        name="chat-failed",
    ),
    path(
        "chat/sessions/<uuid:session_id>/closed/",
        ChatClosedEndpoint.as_view(),
        name="chat-closed",
    ),
    # MachineToken redemption (CLI).
    path(
        "machine-tokens/",
        MachineTokenRedeemEndpoint.as_view(),
        name="machine-token-redeem",
    ),
]
