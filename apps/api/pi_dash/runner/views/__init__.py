# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from .approvals import ApprovalDecideEndpoint, ApprovalListEndpoint
from .machine_tokens import (
    MachineTokenListCreateEndpoint,
    MachineTokenRevokeEndpoint,
    TokenRunnerCreateEndpoint,
)
from .metrics import MetricsEndpoint
from .pods import PodDetailEndpoint, PodListEndpoint
from .register import (
    HealthEndpoint,
    RegisterEndpoint,
    RegistrationTokenCreateEndpoint,
    RunnerDeregisterEndpoint,
    RunnerLinkToTokenEndpoint,
    RunnerRotateEndpoint,
)
from .runners import RunnerDetailEndpoint, RunnerListEndpoint, RunnerRevokeEndpoint
from .runs import (
    AgentRunCancelEndpoint,
    AgentRunDetailEndpoint,
    AgentRunListEndpoint,
    AgentRunReleasePinEndpoint,
)

__all__ = [
    "ApprovalDecideEndpoint",
    "ApprovalListEndpoint",
    "HealthEndpoint",
    "MachineTokenListCreateEndpoint",
    "MachineTokenRevokeEndpoint",
    "TokenRunnerCreateEndpoint",
    "MetricsEndpoint",
    "PodDetailEndpoint",
    "PodListEndpoint",
    "RegisterEndpoint",
    "RegistrationTokenCreateEndpoint",
    "RunnerDeregisterEndpoint",
    "RunnerLinkToTokenEndpoint",
    "RunnerRotateEndpoint",
    "RunnerDetailEndpoint",
    "RunnerListEndpoint",
    "RunnerRevokeEndpoint",
    "AgentRunCancelEndpoint",
    "AgentRunDetailEndpoint",
    "AgentRunListEndpoint",
    "AgentRunReleasePinEndpoint",
]
