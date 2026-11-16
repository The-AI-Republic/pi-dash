# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.api.views import (
    ProjectSchedulerBindingAPIEndpoint,
    ProjectSchedulerDetailAPIEndpoint,
    ProjectSchedulerRunNowAPIEndpoint,
    ProjectSchedulerListAPIEndpoint,
    ProjectSchedulerRunsAPIEndpoint,
    WorkspaceSchedulerDetailAPIEndpoint,
    WorkspaceSchedulerListAPIEndpoint,
)

urlpatterns = [
    path(
        "workspaces/<str:slug>/schedulers/",
        WorkspaceSchedulerListAPIEndpoint.as_view(http_method_names=["get", "post"]),
        name="workspace-schedulers",
    ),
    path(
        "workspaces/<str:slug>/projects/<str:project_id>/schedulers/",
        ProjectSchedulerListAPIEndpoint.as_view(http_method_names=["get", "post"]),
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
    # Writes (PDASHOSS01-278, -318). POST on the two list routes above
    # creates a definition and installs one on a project. The project-scoped
    # detail route stays GET-only: the install is edited and uninstalled
    # through /binding/ and fired through /run-now/, and the shared
    # definition is edited and deleted through its workspace-scoped route.
    path(
        "workspaces/<str:slug>/schedulers/<uuid:scheduler_id>/",
        WorkspaceSchedulerDetailAPIEndpoint.as_view(http_method_names=["patch", "delete"]),
        name="workspace-scheduler-detail",
    ),
    path(
        "workspaces/<str:slug>/projects/<str:project_id>/schedulers/<uuid:scheduler_id>/binding/",
        ProjectSchedulerBindingAPIEndpoint.as_view(http_method_names=["patch", "delete"]),
        name="project-scheduler-binding",
    ),
    path(
        "workspaces/<str:slug>/projects/<str:project_id>/schedulers/<uuid:scheduler_id>/run-now/",
        ProjectSchedulerRunNowAPIEndpoint.as_view(http_method_names=["post"]),
        name="project-scheduler-run-now",
    ),
]
