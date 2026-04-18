# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path, include
from rest_framework.routers import DefaultRouter

from apple_pi_dash.api.views import StickyViewSet


router = DefaultRouter()
router.register(r"stickies", StickyViewSet, basename="workspace-stickies")

urlpatterns = [
    path("workspaces/<str:slug>/", include(router.urls)),
]
