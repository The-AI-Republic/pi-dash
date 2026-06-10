# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Backfill the ``fable-security-audit`` builtin scheduler into every
existing workspace's catalog.

New workspaces get the full BUILTINS catalog via the ``Workspace``
post_save signal; this migration is the one-time backfill so existing
workspaces end up with the same new template in their schedulers tab.

Seeds ONLY the ``fable-security-audit`` slug — unlike the original
``0133`` seed it deliberately does not re-touch other builtins, so any
per-workspace edits to ``security-audit`` are left intact.

Idempotent: if the post_save signal already created the row between
deploy and this migration running, the existing row is refreshed rather
than failing the ``(workspace, slug)`` unique constraint.

See ``.ai_design/project_scheduler/design.md`` §6.6.
"""

from django.db import migrations

TARGET_SLUG = "fable-security-audit"


def _seed_fable_audit(apps, schema_editor):
    Workspace = apps.get_model("db", "Workspace")
    Scheduler = apps.get_model("db", "Scheduler")

    # Pull the prompt/name/description from the single source of truth so
    # the text lives in exactly one place (the builtins registry).
    from pi_dash.scheduler.builtins import BUILTINS

    builtin = next((b for b in BUILTINS if b.slug == TARGET_SLUG), None)
    if builtin is None:
        # Builtin was removed from the registry before this migration ran;
        # nothing to seed.
        return

    for workspace in Workspace.objects.filter(deleted_at__isnull=True).iterator():
        existing = Scheduler.objects.filter(
            workspace=workspace,
            slug=builtin.slug,
            deleted_at__isnull=True,
        ).first()
        if existing is not None:
            existing.name = builtin.name
            existing.description = builtin.description
            existing.prompt = builtin.prompt
            existing.source = "builtin"
            existing.save(
                update_fields=["name", "description", "prompt", "source", "updated_at"]
            )
        else:
            Scheduler.objects.create(
                workspace=workspace,
                slug=builtin.slug,
                name=builtin.name,
                description=builtin.description,
                prompt=builtin.prompt,
                source="builtin",
            )


def _noop_reverse(apps, schema_editor):
    # Forward seed is idempotent; reverse intentionally leaves the rows in
    # place so a rollback doesn't strip data the post_save signal would
    # also have created. Operators wanting to purge can soft-delete the
    # `fable-security-audit` rows after rolling back.
    pass


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0144_issuecomment_speaker_metadata"),
    ]

    operations = [
        migrations.RunPython(_seed_fable_audit, _noop_reverse),
    ]
