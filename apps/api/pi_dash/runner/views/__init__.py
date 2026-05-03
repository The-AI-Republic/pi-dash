# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from .approvals import ApprovalDecideEndpoint, ApprovalListEndpoint
from .enrollment import (
    MachineTokenRedeemEndpoint,
    MachineTokenTicketEndpoint,
    RunnerEnrollEndpoint,
    RunnerInviteEndpoint,
    RunnerRefreshEndpoint,
)
from .metrics import MetricsEndpoint
from .pods import PodDetailEndpoint, PodListEndpoint
from .projects import ProjectListEndpoint
from .register import HealthEndpoint
from .run_endpoints import (
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
)
from .runners import RunnerDetailEndpoint, RunnerListEndpoint
from .runs import (
    AgentRunCancelEndpoint,
    AgentRunDetailEndpoint,
    AgentRunListEndpoint,
    AgentRunReleasePinEndpoint,
)
from .sessions import (
    RunnerSessionDeleteEndpoint,
    RunnerSessionOpenEndpoint,
    RunnerSessionPollEndpoint,
)

__all__ = [
    "ApprovalDecideEndpoint",
    "ApprovalListEndpoint",
    "MachineTokenRedeemEndpoint",
    "MachineTokenTicketEndpoint",
    "RunnerEnrollEndpoint",
    "RunnerInviteEndpoint",
    "RunnerRefreshEndpoint",
    "HealthEndpoint",
    "MetricsEndpoint",
    "PodDetailEndpoint",
    "PodListEndpoint",
    "ProjectListEndpoint",
    "RunnerDetailEndpoint",
    "RunnerListEndpoint",
    "AgentRunCancelEndpoint",
    "AgentRunDetailEndpoint",
    "AgentRunListEndpoint",
    "AgentRunReleasePinEndpoint",
    "RunAcceptEndpoint",
    "RunApprovalEndpoint",
    "RunAwaitingReauthEndpoint",
    "RunCancelledEndpoint",
    "RunCompletedEndpoint",
    "RunEventEndpoint",
    "RunFailedEndpoint",
    "RunPausedEndpoint",
    "RunResumedEndpoint",
    "RunStartedEndpoint",
    "RunStreamUpgradeEndpoint",
    "RunnerSessionDeleteEndpoint",
    "RunnerSessionOpenEndpoint",
    "RunnerSessionPollEndpoint",
]
