# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.core.management.base import BaseCommand

from pi_dash.prompting.seed import seed_review_template


class Command(BaseCommand):
    help = (
        "Refresh the global ``review`` PromptTemplate from the body in "
        "pi_dash.prompting.seed.REVIEW_TEMPLATE_BODY. Does not touch "
        "workspace-scoped templates."
    )

    def add_arguments(self, parser) -> None:
        parser.add_argument(
            "--force",
            action="store_true",
            help="Overwrite the body of the existing global review row.",
        )

    def handle(self, *args, force: bool = False, **options) -> None:
        result = seed_review_template(force=force)
        self.stdout.write(self.style.SUCCESS(f"review template: {result}"))
