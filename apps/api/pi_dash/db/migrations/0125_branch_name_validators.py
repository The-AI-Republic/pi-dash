# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.core import validators
from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0124_issue_git_work_branch"),
    ]

    operations = [
        migrations.AlterField(
            model_name="project",
            name="base_branch",
            field=models.CharField(
                blank=True,
                default="main",
                max_length=128,
                validators=[
                    validators.RegexValidator(
                        message=(
                            "Branch name may contain only letters, numbers, and . _ / -"
                        ),
                        regex="^[A-Za-z0-9._/-]*$",
                    )
                ],
            ),
        ),
        migrations.AlterField(
            model_name="issue",
            name="git_work_branch",
            field=models.CharField(
                blank=True,
                default="",
                max_length=128,
                validators=[
                    validators.RegexValidator(
                        message=(
                            "Branch name may contain only letters, numbers, and . _ / -"
                        ),
                        regex="^[A-Za-z0-9._/-]*$",
                    )
                ],
            ),
        ),
    ]
