# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.prompting.views import PromptTemplatePreviewEndpoint

app_name = "prompting"

urlpatterns = [
    path(
        "workspaces/<slug:slug>/prompt-templates/<uuid:template_id>/preview",
        PromptTemplatePreviewEndpoint.as_view(),
        name="prompt-template-preview",
    ),
]
