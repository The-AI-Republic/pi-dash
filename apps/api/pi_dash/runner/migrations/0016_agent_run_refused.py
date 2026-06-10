# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record safety-classifier refusals as a first-class AgentRun outcome.

Adds the ``refused`` terminal status and a ``refusal_category`` column so a
model decline (e.g. Claude Fable 5 cyber/bio) is queryable separately from a
generic FAILED. See ``runner/services/run_lifecycle.py`` and the
``RunFailedEndpoint`` ``reason="refusal"`` branch.
"""

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("runner", "0015_dev_machine_visibility"),
    ]

    operations = [
        migrations.AddField(
            model_name="agentrun",
            name="refusal_category",
            field=models.CharField(
                blank=True,
                choices=[
                    ("cyber", "Cyber"),
                    ("bio", "Bio"),
                    ("reasoning_extraction", "Reasoning Extraction"),
                    ("unknown", "Unknown"),
                ],
                default="",
                max_length=32,
            ),
        ),
        migrations.AlterField(
            model_name="agentrun",
            name="status",
            field=models.CharField(
                choices=[
                    ("queued", "Queued"),
                    ("assigned", "Assigned"),
                    ("running", "Running"),
                    ("awaiting_approval", "Awaiting Approval"),
                    ("awaiting_reauth", "Awaiting Reauth"),
                    ("paused_awaiting_input", "Paused — Awaiting Input"),
                    ("blocked", "Blocked"),
                    ("completed", "Completed"),
                    ("failed", "Failed"),
                    ("cancelled", "Cancelled"),
                    ("refused", "Refused"),
                ],
                db_index=True,
                default="queued",
                max_length=24,
            ),
        ),
    ]
