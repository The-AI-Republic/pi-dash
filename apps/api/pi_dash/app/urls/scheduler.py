# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.app.views.scheduler.views import (
    ProjectSchedulerBindingDetailEndpoint,
    ProjectSchedulerBindingListEndpoint,
    WorkspaceSchedulerDetailEndpoint,
    WorkspaceSchedulerListEndpoint,
)


urlpatterns = [
    # Workspace-level: scheduler-definition CRUD (workspace admin).
    path(
        "workspaces/<str:slug>/schedulers/",
        WorkspaceSchedulerListEndpoint.as_view(),
        name="workspace-schedulers-list",
    ),
    path(
        "workspaces/<str:slug>/schedulers/<uuid:scheduler_id>/",
        WorkspaceSchedulerDetailEndpoint.as_view(),
        name="workspace-schedulers-detail",
    ),
    # Project-level: scheduler-binding CRUD (project admin).
    path(
        "workspaces/<str:slug>/projects/<uuid:project_id>/scheduler-bindings/",
        ProjectSchedulerBindingListEndpoint.as_view(),
        name="project-scheduler-bindings-list",
    ),
    path(
        "workspaces/<str:slug>/projects/<uuid:project_id>/scheduler-bindings/<uuid:binding_id>/",
        ProjectSchedulerBindingDetailEndpoint.as_view(),
        name="project-scheduler-bindings-detail",
    ),
]
