# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.apps import AppConfig


class OrchestrationConfig(AppConfig):
    name = "pi_dash.orchestration"
    label = "orchestration"
    verbose_name = "Pi Dash Orchestration"
    default_auto_field = "django.db.models.BigAutoField"

    def ready(self) -> None:
        # Importing the signals module wires up the state-transition hook.
        from pi_dash.orchestration import signals  # noqa: F401
