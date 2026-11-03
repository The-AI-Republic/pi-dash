# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.runner.admin_views import InstanceRunnerListEndpoint

# Mounted under /api/instances/runners/ (see license/urls.py).
urlpatterns = [
    path("", InstanceRunnerListEndpoint.as_view(), name="instance-runners"),
]
