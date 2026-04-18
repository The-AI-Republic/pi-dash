# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""apple pi dash URL Configuration"""

from django.conf import settings
from django.urls import include, path, re_path
from drf_spectacular.views import (
    SpectacularAPIView,
    SpectacularRedocView,
    SpectacularSwaggerView,
)

handler404 = "apple_pi_dash.app.views.error_404.custom_404_view"

urlpatterns = [
    path("api/", include("apple_pi_dash.app.urls")),
    path("api/public/", include("apple_pi_dash.space.urls")),
    path("api/instances/", include("apple_pi_dash.license.urls")),
    path("api/v1/", include("apple_pi_dash.api.urls")),
    path("auth/", include("apple_pi_dash.authentication.urls")),
    path("", include("apple_pi_dash.web.urls")),
]

if settings.ENABLE_DRF_SPECTACULAR:
    urlpatterns += [
        path("api/schema/", SpectacularAPIView.as_view(), name="schema"),
        path(
            "api/schema/swagger-ui/",
            SpectacularSwaggerView.as_view(url_name="schema"),
            name="swagger-ui",
        ),
        path(
            "api/schema/redoc/",
            SpectacularRedocView.as_view(url_name="schema"),
            name="redoc",
        ),
    ]

if settings.DEBUG:
    try:
        import debug_toolbar

        urlpatterns = [re_path(r"^__debug__/", include(debug_toolbar.urls))] + urlpatterns
    except ImportError:
        pass
