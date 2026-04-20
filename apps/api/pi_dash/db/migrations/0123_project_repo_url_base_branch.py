# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0122_project_members_can_edit_states"),
    ]

    operations = [
        migrations.AddField(
            model_name="project",
            name="repo_url",
            field=models.CharField(blank=True, default="", max_length=512),
        ),
        migrations.AddField(
            model_name="project",
            name="base_branch",
            field=models.CharField(blank=True, default="main", max_length=128),
        ),
    ]
