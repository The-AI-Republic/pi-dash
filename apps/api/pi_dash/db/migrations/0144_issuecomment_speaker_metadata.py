# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0143_issue_comment_fts_idx"),
    ]

    operations = [
        migrations.AddField(
            model_name="issuecomment",
            name="speaker_type",
            field=models.CharField(
                choices=[
                    ("human", "Human"),
                    ("agent", "Agent"),
                    ("system", "System"),
                    ("integration", "Integration"),
                ],
                default="human",
                max_length=32,
            ),
        ),
        migrations.AddField(
            model_name="issuecomment",
            name="speaker_label",
            field=models.CharField(blank=True, default="", max_length=128),
        ),
        migrations.AddField(
            model_name="issuecomment",
            name="speaker_agent_run_id",
            field=models.UUIDField(blank=True, null=True),
        ),
    ]
