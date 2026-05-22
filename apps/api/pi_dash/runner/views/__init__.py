# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from .approvals import ApprovalDecideEndpoint, ApprovalListEndpoint
from .chat import (
    AgentChatApprovalDecideEndpoint,
    AgentChatApprovalListEndpoint,
    AgentChatCancelEndpoint,
    AgentChatCloseEndpoint,
    AgentChatMessageListEndpoint,
    AgentChatSessionDetailEndpoint,
    AgentChatSessionListEndpoint,
    ChatApprovalEndpoint,
    ChatClosedEndpoint,
    ChatEventEndpoint,
    ChatFailedEndpoint,
    ChatMessageCompleteEndpoint,
    ChatMessageStartedEndpoint,
    ChatStartedEndpoint,
    chat_event_stream,
)
from .enrollment import (
    MachineTokenRedeemEndpoint,
    MachineTokenTicketEndpoint,
    RunnerCreateEndpoint,
    RunnerEnrollEndpoint,
    RunnerInviteEndpoint,
    RunnerRefreshEndpoint,
    RunnerReviveEndpoint,
    RunnerSelfRevokeEndpoint,
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
from .runners import (
    RunnerDetailEndpoint,
    RunnerListEndpoint,
    RunnerRevokeEndpoint,
)
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
    "AgentChatApprovalDecideEndpoint",
    "AgentChatApprovalListEndpoint",
    "AgentChatCancelEndpoint",
    "AgentChatCloseEndpoint",
    "AgentChatMessageListEndpoint",
    "AgentChatSessionDetailEndpoint",
    "AgentChatSessionListEndpoint",
    "ChatApprovalEndpoint",
    "ChatClosedEndpoint",
    "ChatEventEndpoint",
    "ChatFailedEndpoint",
    "ChatMessageCompleteEndpoint",
    "ChatMessageStartedEndpoint",
    "ChatStartedEndpoint",
    "chat_event_stream",
    "MachineTokenRedeemEndpoint",
    "MachineTokenTicketEndpoint",
    "RunnerCreateEndpoint",
    "RunnerEnrollEndpoint",
    "RunnerInviteEndpoint",
    "RunnerRefreshEndpoint",
    "RunnerReviveEndpoint",
    "RunnerRevokeEndpoint",
    "RunnerSelfRevokeEndpoint",
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
