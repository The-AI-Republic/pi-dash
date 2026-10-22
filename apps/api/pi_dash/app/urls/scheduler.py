# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.app.views.scheduler.occurrences import ProjectSchedulerOccurrencesEndpoint
from pi_dash.app.views.scheduler.views import (
    ProjectSchedulerBindingDetailEndpoint,
    ProjectSchedulerBindingListEndpoint,
    ProjectSchedulerBindingRunsEndpoint,
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
        "workspaces/<str:slug>/projects/<str:project_id>/scheduler-bindings/",
        ProjectSchedulerBindingListEndpoint.as_view(),
        name="project-scheduler-bindings-list",
    ),
    path(
        "workspaces/<str:slug>/projects/<str:project_id>/scheduler-bindings/<uuid:binding_id>/",
        ProjectSchedulerBindingDetailEndpoint.as_view(),
        name="project-scheduler-bindings-detail",
    ),
    # Run history for one binding (detail-page drilldown).
    path(
        "workspaces/<str:slug>/projects/<str:project_id>/scheduler-bindings/<uuid:binding_id>/runs/",
        ProjectSchedulerBindingRunsEndpoint.as_view(),
        name="project-scheduler-bindings-runs",
    ),
    # Project-level: calendar occurrences (PR2 — see
    # .ai_design/project_scheduler_calendar/decisions.md §3).
    path(
        "workspaces/<str:slug>/projects/<str:project_id>/scheduler-bindings/occurrences/",
        ProjectSchedulerOccurrencesEndpoint.as_view(),
        name="project-scheduler-bindings-occurrences",
    ),
]
