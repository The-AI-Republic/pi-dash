# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Work-type axis (PDASHOSS01-234).

``Project.default_work_type`` selects which per-work-type prompt guidance
fills the stage recipes' slots; nullable ``Issue.work_type`` overrides it
per issue (NULL = inherit). The data step stamps ``software`` on every
pre-existing project so their rendered prompts are unchanged — new projects
resolve their default at creation ("software" when a repo is bound,
"general" otherwise, in ``Project.save``).
"""

from django.db import migrations, models


def _stamp_existing_projects_software(apps, schema_editor):
    Project = apps.get_model("db", "Project")
    Project.objects.filter(default_work_type="").update(default_work_type="software")


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0167_wait_budget"),
    ]

    operations = [
        migrations.AddField(
            model_name="issue",
            name="work_type",
            field=models.CharField(blank=True, max_length=32, null=True),
        ),
        migrations.AddField(
            model_name="project",
            name="default_work_type",
            field=models.CharField(blank=True, default="", max_length=32),
        ),
        migrations.RunPython(
            _stamp_existing_projects_software,
            # Reverse: the field is dropped with the migration; nothing to undo.
            migrations.RunPython.noop,
        ),
    ]
