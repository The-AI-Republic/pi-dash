# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add the ``Paused`` default state and hide ``Todo`` (NEWPIDASH-417).

Schema: ``State.is_hidden`` (boolean, default ``False``); the default
manager (``StateManager``) now excludes hidden states the same way it
excludes the ``triage`` group, so ``Todo`` disappears from normal
listings while id lookups and issue serialization (which read through
``all_state_objects`` or the row itself) keep resolving it.

Data: ``Paused`` (``unstarted``, sequence 30000, between ``Todo``
25000 and ``In Progress`` 35000) is inserted into every project that
does not already have a live state named ``Paused`` — the unique
name-per-project constraint is respected, and projects backfilled by
the legacy ``0128_paused_state`` migration (``backlog`` group,
sequence 17500) keep their row as-is. Every live ``Todo`` state is
flagged hidden. No issue rows change. Color reuses the legacy Paused
color (``#DC2626``).

Idempotent: re-running inserts nothing new and re-applies the flag.
Reverse removes only the rows this migration inserted (scoped to the
new ``unstarted``/30000 shape, so legacy 0128 rows are never touched)
and un-hides ``Todo``.
"""

from django.db import migrations, models


PAUSED = {
    "name": "Paused",
    "color": "#DC2626",
    "sequence": 30000.0,
    "group": "unstarted",
    "default": False,
    "is_hidden": False,
}


def backfill_paused_and_hide_todo(apps, schema_editor):
    # Only the default manager (`objects`) survives onto historical
    # models; the named managers (`all_state_objects`) do not.
    Project = apps.get_model("db", "Project")
    State = apps.get_model("db", "State")

    for project in Project.objects.all().iterator(chunk_size=500):
        if State.objects.filter(
            name=PAUSED["name"], project=project, deleted_at__isnull=True
        ).exists():
            continue
        State.objects.create(
            name=PAUSED["name"],
            color=PAUSED["color"],
            description="",
            slug="",
            sequence=PAUSED["sequence"],
            group=PAUSED["group"],
            default=PAUSED["default"],
            is_triage=False,
            is_hidden=PAUSED["is_hidden"],
            project=project,
            workspace=project.workspace,
            created_by=None,
            updated_by=None,
        )

    State.objects.filter(name="Todo", deleted_at__isnull=True).update(
        is_hidden=True
    )


def remove_paused_and_unhide_todo(apps, schema_editor):
    State = apps.get_model("db", "State")
    State.objects.filter(
        name=PAUSED["name"],
        group=PAUSED["group"],
        sequence=PAUSED["sequence"],
        deleted_at__isnull=True,
    ).delete()
    State.objects.filter(name="Todo", deleted_at__isnull=True).update(
        is_hidden=False
    )


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0183_merge_ai_completed_and_project_settings"),
    ]

    operations = [
        # The project rule requires a database default for any NOT NULL
        # column (the shared database is also written by Rust and raw
        # SQL, which may omit the column): the DDL keeps
        # ``DEFAULT false`` while the state half records the model field.
        migrations.SeparateDatabaseAndState(
            database_operations=[
                migrations.RunSQL(
                    sql='ALTER TABLE "states" ADD COLUMN "is_hidden" boolean DEFAULT false NOT NULL;',
                    reverse_sql='ALTER TABLE "states" DROP COLUMN "is_hidden";',
                ),
            ],
            state_operations=[
                migrations.AddField(
                    model_name="state",
                    name="is_hidden",
                    field=models.BooleanField(default=False),
                ),
            ],
        ),
        migrations.RunPython(
            backfill_paused_and_hide_todo,
            remove_paused_and_unhide_todo,
        ),
    ]
