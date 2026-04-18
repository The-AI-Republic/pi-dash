# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.apps import AppConfig


class RunnerConfig(AppConfig):
    name = "apple_pi_dash.runner"
    label = "runner"
    verbose_name = "Apple Pi Dash Runner"
    default_auto_field = "django.db.models.BigAutoField"
