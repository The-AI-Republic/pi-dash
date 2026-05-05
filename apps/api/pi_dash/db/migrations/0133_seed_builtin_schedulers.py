# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Seed builtin schedulers (e.g. ``security-audit``) into every existing
workspace's catalog.

Idempotent: calling this migration repeatedly (or against a workspace
that was already seeded by the post_save signal) updates the existing
row rather than failing the unique constraint.

See ``.ai_design/project_scheduler/design.md`` §6.6.
"""

from django.db import migrations


def _seed_all_workspaces(apps, schema_editor):
    Workspace = apps.get_model("db", "Workspace")
    Scheduler = apps.get_model("db", "Scheduler")

    # Inline a slim version of ensure_builtin_schedulers — historical
    # migrations must not import live model code (it can drift away from
    # the migration's frozen schema). The fields used here are the ones
    # the migration just created.
    from pi_dash.scheduler.builtins import BUILTINS

    for workspace in Workspace.objects.filter(deleted_at__isnull=True).iterator():
        for builtin in BUILTINS:
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
    # Forward seed is idempotent; reverse intentionally leaves the rows
    # in place so a rollback doesn't strip data the post_save signal
    # would also have created. Operators wanting to fully purge can
    # truncate the table after rolling back.
    pass


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0132_project_scheduler_mvp"),
    ]

    operations = [
        migrations.RunPython(_seed_all_workspaces, _noop_reverse),
    ]
