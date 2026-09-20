# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.api.views import (
    PageListAPIEndpoint,
    PageDetailAPIEndpoint,
)

urlpatterns = [
    path(
        "workspaces/<str:slug>/projects/<str:project_id>/pages/",
        PageListAPIEndpoint.as_view(http_method_names=["get"]),
        name="pages",
    ),
    path(
        "workspaces/<str:slug>/projects/<str:project_id>/pages/<uuid:page_id>/",
        PageDetailAPIEndpoint.as_view(http_method_names=["get"]),
        name="pages-detail",
    ),
]
