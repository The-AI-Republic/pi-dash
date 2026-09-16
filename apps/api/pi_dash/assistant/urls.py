# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.assistant.views.events import assistant_event_stream
from pi_dash.assistant.views.agent_profile import (
    AgentModelProfileEndpoint,
    AgentModelTokenEndpoint,
)
from pi_dash.assistant.views.llm_config import (
    AssistantGenerateTitleEndpoint,
    UserLLMConfigEndpoint,
    UserLLMConfigTestEndpoint,
)
from pi_dash.assistant.views.mcp_servers import (
    AssistantMCPServerDetailEndpoint,
    AssistantMCPServerListCreateEndpoint,
)
from pi_dash.assistant.views.messages import (
    AssistantCancelEndpoint,
    AssistantMessageListCreateEndpoint,
)
from pi_dash.assistant.views.threads import (
    AssistantThreadDetailEndpoint,
    AssistantThreadListCreateEndpoint,
)

_ASSIST = "workspaces/<str:slug>/ai-assistant"

urlpatterns = [
    path(f"{_ASSIST}/threads/", AssistantThreadListCreateEndpoint.as_view(), name="assistant-threads"),
    path(
        f"{_ASSIST}/threads/<uuid:thread_id>/",
        AssistantThreadDetailEndpoint.as_view(),
        name="assistant-thread-detail",
    ),
    path(
        f"{_ASSIST}/threads/<uuid:thread_id>/messages/",
        AssistantMessageListCreateEndpoint.as_view(),
        name="assistant-messages",
    ),
    path(
        f"{_ASSIST}/threads/<uuid:thread_id>/events/",
        assistant_event_stream,
        name="assistant-events",
    ),
    path(
        f"{_ASSIST}/threads/<uuid:thread_id>/cancel/",
        AssistantCancelEndpoint.as_view(),
        name="assistant-cancel",
    ),
    path(
        f"{_ASSIST}/generate-title/",
        AssistantGenerateTitleEndpoint.as_view(),
        name="assistant-generate-title",
    ),
    path("users/me/ai-assistant/config/", UserLLMConfigEndpoint.as_view(), name="ai-assistant-config"),
    path(
        "users/me/ai-assistant/config/test/",
        UserLLMConfigTestEndpoint.as_view(),
        name="ai-assistant-config-test",
    ),
    # Desktop-only (IsDesktopSession): what the bundled agent engine should
    # call, and the short-lived credential to call it with.
    path(
        "users/me/ai-assistant/agent-profile/",
        AgentModelProfileEndpoint.as_view(),
        name="ai-assistant-agent-profile",
    ),
    path(
        "users/me/ai-assistant/agent-token/",
        AgentModelTokenEndpoint.as_view(),
        name="ai-assistant-agent-token",
    ),
    path(
        "users/me/ai-assistant/mcp-servers/",
        AssistantMCPServerListCreateEndpoint.as_view(),
        name="ai-assistant-mcp-servers",
    ),
    path(
        "users/me/ai-assistant/mcp-servers/<uuid:server_id>/",
        AssistantMCPServerDetailEndpoint.as_view(),
        name="ai-assistant-mcp-server-detail",
    ),
]
