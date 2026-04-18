# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from apple_pi_dash.runner.consumers import RunnerConsumer

websocket_urlpatterns = [
    path("ws/runner/", RunnerConsumer.as_asgi()),
]
