# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.loop.admin_views import (
    LoopJobDetailEndpoint,
    LoopJobListCreateEndpoint,
    LoopJobTargetsEndpoint,
)

# Mounted under /api/instances/loop/ (see license/urls.py).
urlpatterns = [
    path("jobs/", LoopJobListCreateEndpoint.as_view(), name="loop-jobs"),
    path("jobs/<uuid:pk>/", LoopJobDetailEndpoint.as_view(), name="loop-job-detail"),
    path("jobs/<uuid:pk>/targets/", LoopJobTargetsEndpoint.as_view(), name="loop-job-targets"),
]
