# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0013_agent_chat"),
    ]

    operations = [
        migrations.AddField(
            model_name="agentrun",
            name="input_tokens",
            field=models.BigIntegerField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="agentrun",
            name="llm_model",
            field=models.CharField(blank=True, default="", max_length=128),
        ),
        migrations.AddField(
            model_name="agentrun",
            name="output_tokens",
            field=models.BigIntegerField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="agentrun",
            name="total_tokens",
            field=models.BigIntegerField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="runnerlivestate",
            name="llm_model",
            field=models.CharField(blank=True, max_length=128, null=True),
        ),
    ]
