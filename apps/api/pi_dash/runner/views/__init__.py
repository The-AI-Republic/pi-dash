# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from .approvals import ApprovalDecideEndpoint, ApprovalListEndpoint
from .connections import (
    ConnectionDetailEndpoint,
    ConnectionEnrollEndpoint,
    ConnectionListCreateEndpoint,
    ConnectionRevokeEndpoint,
    ConnectionRunnerDeleteEndpoint,
    ConnectionRunnerListCreateEndpoint,
)
from .metrics import MetricsEndpoint
from .pods import PodDetailEndpoint, PodListEndpoint
from .projects import ProjectListEndpoint
from .register import HealthEndpoint
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
    "ConnectionDetailEndpoint",
    "ConnectionEnrollEndpoint",
    "ConnectionListCreateEndpoint",
    "ConnectionRevokeEndpoint",
    "ConnectionRunnerDeleteEndpoint",
    "ConnectionRunnerListCreateEndpoint",
    "HealthEndpoint",
    "MetricsEndpoint",
    "PodDetailEndpoint",
    "PodListEndpoint",
    "ProjectListEndpoint",
    "RunnerDetailEndpoint",
    "RunnerListEndpoint",
    "RunnerRevokeEndpoint",
    "AgentRunCancelEndpoint",
    "AgentRunDetailEndpoint",
    "AgentRunListEndpoint",
    "AgentRunReleasePinEndpoint",
]
