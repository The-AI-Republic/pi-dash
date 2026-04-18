# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path


from apple_pi_dash.app.views import UnsplashEndpoint
from apple_pi_dash.app.views import GPTIntegrationEndpoint, WorkspaceGPTIntegrationEndpoint


urlpatterns = [
    path("unsplash/", UnsplashEndpoint.as_view(), name="unsplash"),
    path(
        "workspaces/<str:slug>/projects/<uuid:project_id>/ai-assistant/",
        GPTIntegrationEndpoint.as_view(),
        name="importer",
    ),
    path(
        "workspaces/<str:slug>/ai-assistant/",
        WorkspaceGPTIntegrationEndpoint.as_view(),
        name="importer",
    ),
]
