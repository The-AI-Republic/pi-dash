# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.api.views import (
    PageListAPIEndpoint,
    PageDetailAPIEndpoint,
    PageArchiveAPIEndpoint,
)

urlpatterns = [
    path(
        "workspaces/<str:slug>/projects/<str:project_id>/pages/",
        PageListAPIEndpoint.as_view(http_method_names=["get", "post"]),
        name="pages",
    ),
    path(
        "workspaces/<str:slug>/projects/<str:project_id>/pages/<uuid:page_id>/",
        PageDetailAPIEndpoint.as_view(http_method_names=["get", "patch"]),
        name="pages-detail",
    ),
    path(
        "workspaces/<str:slug>/projects/<str:project_id>/pages/<uuid:page_id>/archive/",
        PageArchiveAPIEndpoint.as_view(http_method_names=["post", "delete"]),
        name="pages-archive",
    ),
]
