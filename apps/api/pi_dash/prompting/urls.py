# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.prompting.views import (
    PromptTemplateArchiveEndpoint,
    PromptTemplateDetailEndpoint,
    PromptTemplateListCreateEndpoint,
    PromptTemplatePreviewEndpoint,
)

app_name = "prompting"

urlpatterns = [
    path(
        "workspaces/<slug:slug>/prompt-templates",
        PromptTemplateListCreateEndpoint.as_view(),
        name="prompt-template-list-create",
    ),
    path(
        "workspaces/<slug:slug>/prompt-templates/<uuid:template_id>",
        PromptTemplateDetailEndpoint.as_view(),
        name="prompt-template-detail",
    ),
    path(
        "workspaces/<slug:slug>/prompt-templates/<uuid:template_id>/archive",
        PromptTemplateArchiveEndpoint.as_view(),
        name="prompt-template-archive",
    ),
    path(
        "workspaces/<slug:slug>/prompt-templates/<uuid:template_id>/preview",
        PromptTemplatePreviewEndpoint.as_view(),
        name="prompt-template-preview",
    ),
]
