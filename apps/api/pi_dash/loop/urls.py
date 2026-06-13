# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.loop.views import AutoPMJobEndpoint, AutoPMSettingsEndpoint

urlpatterns = [
    path("users/me/auto-pm/", AutoPMSettingsEndpoint.as_view(), name="auto-pm-settings"),
    path("users/me/auto-pm/jobs/<str:slug>/", AutoPMJobEndpoint.as_view(), name="auto-pm-job"),
]
