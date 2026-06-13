# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Re-validate all active prompt-section overrides (design §6.4).

Run this in the same PR as any change that removes/renames a context variable
(``build_context`` / ``build_scheduler_context``) or flips a section's
customizability. It re-runs the save-time validation over every active
override and sets ``needs_attention=True`` on the ones that would now fail at
render time — it never deletes or deactivates a row.
"""

from __future__ import annotations

from django.core.management.base import BaseCommand

from pi_dash.prompting import registry
from pi_dash.prompting.models import PromptSectionOverride
from pi_dash.prompting.validation import OverrideValidationError, validate_override


class Command(BaseCommand):
    help = "Re-validate active prompt-section overrides; flag broken ones."

    def add_arguments(self, parser):
        parser.add_argument(
            "--clear",
            action="store_true",
            help="Clear needs_attention on overrides that now validate cleanly.",
        )

    def handle(self, *args, **options):
        clear = options["clear"]
        checked = flagged = cleared = 0
        for row in PromptSectionOverride.objects.filter(is_active=True).iterator():
            checked += 1
            # A section that no longer exists (renamed/removed key) is always a
            # problem; flag it without attempting to render.
            broken = row.section_key not in registry.REGISTRY
            detail = "section key no longer exists" if broken else ""
            if not broken:
                try:
                    validate_override(
                        row.section_key,
                        row.body,
                        workspace=row.workspace,
                        user=row.user,
                    )
                except OverrideValidationError as exc:
                    broken = True
                    detail = str(exc)

            if broken and not row.needs_attention:
                row.needs_attention = True
                row.save(update_fields=["needs_attention", "updated_at"])
                flagged += 1
                self.stdout.write(
                    self.style.WARNING(
                        f"flagged {row.workspace_id}/{row.section_key} "
                        f"(user={row.user_id}): {detail}"
                    )
                )
            elif not broken and row.needs_attention and clear:
                row.needs_attention = False
                row.save(update_fields=["needs_attention", "updated_at"])
                cleared += 1

        self.stdout.write(
            self.style.SUCCESS(
                f"checked {checked} active override(s): "
                f"{flagged} newly flagged, {cleared} cleared."
            )
        )
