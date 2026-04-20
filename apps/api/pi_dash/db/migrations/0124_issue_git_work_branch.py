# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0123_project_repo_url_base_branch"),
    ]

    operations = [
        migrations.AddField(
            model_name="issue",
            name="git_work_branch",
            field=models.CharField(blank=True, default="", max_length=128),
        ),
    ]
