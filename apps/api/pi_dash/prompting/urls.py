# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.prompting.views import (
    PromptCompiledEndpoint,
    PromptLabelAssignmentEndpoint,
    PromptLabelDetailEndpoint,
    PromptLabelListEndpoint,
    PromptPreviewEndpoint,
    PromptSectionDetailEndpoint,
    PromptSectionListEndpoint,
)

app_name = "prompting"

urlpatterns = [
    path(
        "workspaces/<slug:slug>/prompt-labels",
        PromptLabelListEndpoint.as_view(),
        name="prompt-label-list",
    ),
    path(
        "workspaces/<slug:slug>/prompt-labels/<uuid:label_id>",
        PromptLabelDetailEndpoint.as_view(),
        name="prompt-label-detail",
    ),
    path(
        "workspaces/<slug:slug>/prompt-labels/<uuid:label_id>/assignments",
        PromptLabelAssignmentEndpoint.as_view(),
        name="prompt-label-assignments",
    ),
    path(
        "workspaces/<slug:slug>/prompt-sections",
        PromptSectionListEndpoint.as_view(),
        name="prompt-section-list",
    ),
    path(
        "workspaces/<slug:slug>/prompt-sections/<str:section_key>",
        PromptSectionDetailEndpoint.as_view(),
        name="prompt-section-detail",
    ),
    path(
        "workspaces/<slug:slug>/prompts/<str:kind>/compiled",
        PromptCompiledEndpoint.as_view(),
        name="prompt-compiled",
    ),
    path(
        "workspaces/<slug:slug>/prompts/<str:kind>/preview",
        PromptPreviewEndpoint.as_view(),
        name="prompt-preview",
    ),
]
