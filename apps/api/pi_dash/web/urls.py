# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path
from pi_dash.web.views import robots_txt, health_check

urlpatterns = [path("robots.txt", robots_txt), path("", health_check)]
