# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.api.views import (
    ProjectSchedulerDetailAPIEndpoint,
    ProjectSchedulerListAPIEndpoint,
    ProjectSchedulerRunsAPIEndpoint,
    WorkspaceSchedulerListAPIEndpoint,
)

urlpatterns = [
    path(
        "workspaces/<str:slug>/schedulers/",
        WorkspaceSchedulerListAPIEndpoint.as_view(http_method_names=["get"]),
        name="workspace-schedulers",
    ),
    path(
        "workspaces/<str:slug>/projects/<str:project_id>/schedulers/",
        ProjectSchedulerListAPIEndpoint.as_view(http_method_names=["get"]),
        name="project-schedulers",
    ),
    path(
        "workspaces/<str:slug>/projects/<str:project_id>/schedulers/<uuid:scheduler_id>/",
        ProjectSchedulerDetailAPIEndpoint.as_view(http_method_names=["get"]),
        name="project-schedulers",
    ),
    path(
        "workspaces/<str:slug>/projects/<str:project_id>/schedulers/<uuid:scheduler_id>/runs/",
        ProjectSchedulerRunsAPIEndpoint.as_view(http_method_names=["get"]),
        name="project-scheduler-runs",
    ),
]
