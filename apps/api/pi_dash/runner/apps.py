# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.apps import AppConfig


class RunnerConfig(AppConfig):
    name = "pi_dash.runner"
    label = "runner"
    verbose_name = "Pi Dash Runner"
    default_auto_field = "django.db.models.BigAutoField"

    def ready(self):
        # Import for side effects: registers post_save signal handlers
        # (workspace auto-pod creation). See runner/signals.py.
        from pi_dash.runner import signals  # noqa: F401
