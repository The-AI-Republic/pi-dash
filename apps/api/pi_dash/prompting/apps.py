# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.apps import AppConfig
from django.db.models.signals import post_migrate


class PromptingConfig(AppConfig):
    name = "pi_dash.prompting"
    label = "prompting"
    verbose_name = "Pi Dash Prompting"
    default_auto_field = "django.db.models.BigAutoField"

    def ready(self) -> None:
        from pi_dash.prompting.seed import seed_default_template_on_migrate

        post_migrate.connect(
            seed_default_template_on_migrate, sender=self, dispatch_uid="prompting.seed_default"
        )
