# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.core.management.base import BaseCommand

from pi_dash.prompting.seed import seed_default_template


class Command(BaseCommand):
    help = (
        "Refresh the global default PromptTemplate from the ordered fragments "
        "in apps/api/pi_dash/prompting/fragments/. Does not touch "
        "workspace-scoped templates."
    )

    def add_arguments(self, parser) -> None:
        parser.add_argument(
            "--force",
            action="store_true",
            help="Overwrite the body of the existing global default row.",
        )

    def handle(self, *args, force: bool = False, **options) -> None:
        result = seed_default_template(force=force)
        self.stdout.write(self.style.SUCCESS(f"default template: {result}"))
