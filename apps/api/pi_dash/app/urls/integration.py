# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.app.views.integration.github import (
    GithubAppCallbackEndpoint,
    GithubAppInstallStartEndpoint,
    GithubAppRefreshEndpoint,
    GithubAppStatusEndpoint,
    GithubAppWebhookEndpoint,
    GithubIntegrationConnectEndpoint,
    GithubIntegrationDisconnectEndpoint,
    GithubIntegrationReposEndpoint,
    GithubIntegrationStatusEndpoint,
    GithubProjectBindEndpoint,
    GithubProjectStatusEndpoint,
)


urlpatterns = [
    # Profile-level GitHub App install flow
    path(
        "users/me/integrations/github/app/",
        GithubAppStatusEndpoint.as_view(),
        name="github-app-status",
    ),
    path(
        "users/me/integrations/github/app/install/",
        GithubAppInstallStartEndpoint.as_view(),
        name="github-app-install-start",
    ),
    path(
        "users/me/integrations/github/app/refresh/",
        GithubAppRefreshEndpoint.as_view(),
        name="github-app-refresh",
    ),
    path(
        "integrations/github/app/callback/",
        GithubAppCallbackEndpoint.as_view(),
        name="github-app-callback",
    ),
    path(
        "integrations/github/app/webhook/",
        GithubAppWebhookEndpoint.as_view(),
        name="github-app-webhook",
    ),
    # Workspace-level
    path(
        "workspaces/<str:slug>/integrations/github/",
        GithubIntegrationStatusEndpoint.as_view(),
        name="github-integration-status",
    ),
    path(
        "workspaces/<str:slug>/integrations/github/connect/",
        GithubIntegrationConnectEndpoint.as_view(),
        name="github-integration-connect",
    ),
    path(
        "workspaces/<str:slug>/integrations/github/disconnect/",
        GithubIntegrationDisconnectEndpoint.as_view(),
        name="github-integration-disconnect",
    ),
    path(
        "workspaces/<str:slug>/integrations/github/repos/",
        GithubIntegrationReposEndpoint.as_view(),
        name="github-integration-repos",
    ),
    # Project-level
    path(
        "workspaces/<str:slug>/projects/<str:project_id>/github/",
        GithubProjectStatusEndpoint.as_view(),
        name="github-project-status",
    ),
    path(
        "workspaces/<str:slug>/projects/<str:project_id>/github/bind/",
        GithubProjectBindEndpoint.as_view(),
        name="github-project-bind",
    ),
]
