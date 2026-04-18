# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from apple_pi_dash.api.views import UserEndpoint

urlpatterns = [
    path(
        "users/me/",
        UserEndpoint.as_view(http_method_names=["get"]),
        name="users",
    ),
]
