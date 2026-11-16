# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.api.views import RunnerDeleteEndpoint, RunnerListAPIEndpoint

urlpatterns = [
    path(
        "runners/<uuid:runner_id>/",
        RunnerDeleteEndpoint.as_view(http_method_names=["delete"]),
        name="api-runner-delete",
    ),
    path(
        "workspaces/<str:slug>/runners/",
        RunnerListAPIEndpoint.as_view(http_method_names=["get"]),
        name="api-runner-list",
    ),
]
