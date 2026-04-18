# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from apple_pi_dash.app.views import ExportIssuesEndpoint


urlpatterns = [
    path(
        "workspaces/<str:slug>/export-issues/",
        ExportIssuesEndpoint.as_view(),
        name="export-issues",
    ),
]
