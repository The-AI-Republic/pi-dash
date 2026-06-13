# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.assistant.views.events import assistant_event_stream
from pi_dash.assistant.views.llm_config import (
    UserLLMConfigEndpoint,
    UserLLMConfigTestEndpoint,
)
from pi_dash.assistant.views.messages import (
    AssistantCancelEndpoint,
    AssistantMessageListCreateEndpoint,
)
from pi_dash.assistant.views.threads import (
    AssistantThreadDetailEndpoint,
    AssistantThreadListCreateEndpoint,
)

_ASSIST = "workspaces/<str:slug>/assistant"

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
    path("users/me/llm-config/", UserLLMConfigEndpoint.as_view(), name="assistant-llm-config"),
    path("users/me/llm-config/test/", UserLLMConfigTestEndpoint.as_view(), name="assistant-llm-config-test"),
]
