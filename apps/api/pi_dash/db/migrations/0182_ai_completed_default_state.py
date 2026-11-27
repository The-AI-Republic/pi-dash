# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Backfill the "AI Completed" default state for existing projects.

NEWPIDASH-305 adds "AI Completed" (group ``completed``, sequence 43750,
between "In Test" 42500 and "Done" 45000) to ``DEFAULT_STATES``
(``db/models/state.py``), which only seeds new projects. Existing
projects get it from this data migration. A project that already has a
live state named "AI Completed" is skipped. Field values mirror the
``State.objects.bulk_create`` seeding in project creation
(``app/views/project/base.py``): ``slug`` stays ``""`` (``bulk_create``
bypasses ``State.save()``), ``description`` is ``""``, ``is_triage`` is
``False`` and ``default`` is ``False``. ``created_by`` is NULL — there
is no actor in a migration run.

Idempotent: re-running does not duplicate the row.
"""

from django.db import migrations


AI_COMPLETED = {
    "name": "AI Completed",
    "color": "#30A46C",
    "sequence": 43750.0,
    "group": "completed",
    "default": False,
}


def backfill_ai_completed(apps, schema_editor):
    # Only the default manager (`objects`) survives onto historical
    # models; the named managers (`all_state_objects`) do not.
    Project = apps.get_model("db", "Project")
    State = apps.get_model("db", "State")

    for project in Project.objects.all().iterator(chunk_size=500):
        if State.objects.filter(
            name="AI Completed", project=project, deleted_at__isnull=True
        ).exists():
            continue
        State.objects.create(
            name=AI_COMPLETED["name"],
            color=AI_COMPLETED["color"],
            description="",
            slug="",
            sequence=AI_COMPLETED["sequence"],
            group=AI_COMPLETED["group"],
            default=AI_COMPLETED["default"],
            is_triage=False,
            project=project,
            workspace=project.workspace,
            created_by=None,
            updated_by=None,
        )


def remove_ai_completed(apps, schema_editor):
    State = apps.get_model("db", "State")
    State.objects.filter(
        name="AI Completed", deleted_at__isnull=True
    ).delete()


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0181_work_type_writer_defaults"),
    ]

    operations = [
        migrations.RunPython(backfill_ai_completed, remove_ai_completed),
    ]
