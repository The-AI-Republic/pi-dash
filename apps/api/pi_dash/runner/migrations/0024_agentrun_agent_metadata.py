# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add ``AgentRun.agent_metadata`` — a general-purpose, write-once-per-attempt
JSON column for run metadata that has no owned single-producer/consumer
contract. Used to durably record the runner-reported local agent session id /
thread id / agent kind at run start; unlike ``thread_id`` it is never cleared
on retry, so a failed attempt's session id survives for audit."""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0023_one_active_includes_cancel_requested"),
    ]

    operations = [
        migrations.AddField(
            model_name="agentrun",
            name="agent_metadata",
            field=models.JSONField(blank=True, default=dict),
        ),
    ]
