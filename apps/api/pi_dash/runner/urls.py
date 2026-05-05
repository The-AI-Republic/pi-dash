# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""External runner-facing API (daemon traffic) — mounted at ``/api/v1/runner/``.

See ``.ai_design/move_to_https/design.md`` §5 / §7. Per-runner trust:
the daemon authenticates as a Runner with a per-runner refresh+access
token pair (no Connection layer), opens one session per runner, and
long-polls that session for control-plane messages.
"""

from django.urls import path

from pi_dash.runner.views import (
    HealthEndpoint,
    MachineTokenRedeemEndpoint,
    MetricsEndpoint,
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
    RunnerSessionDeleteEndpoint,
    RunnerSessionOpenEndpoint,
    RunnerSessionPollEndpoint,
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
    path(
        "runners/<uuid:runner_id>/refresh/",
        RunnerRefreshEndpoint.as_view(),
        name="runner-refresh",
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
        RunnerSessionPollEndpoint.as_view(),
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
    # MachineToken redemption (CLI).
    path(
        "machine-tokens/",
        MachineTokenRedeemEndpoint.as_view(),
        name="machine-token-redeem",
    ),
]
