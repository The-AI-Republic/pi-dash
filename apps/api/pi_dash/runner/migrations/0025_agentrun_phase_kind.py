# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""``AgentRun.phase_kind`` — the prompt kind a run was rendered for.

Read by the ticker's outcome guard (``.ai_design/ticking_relevance/design.md``
§7): a ``done`` reported by a run whose stage the issue has since left must
not stop the clock that is now set for the *next* stage.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0024_managed_runner_provisioning"),
    ]

    operations = [
        migrations.AddField(
            model_name="agentrun",
            name="phase_kind",
            field=models.CharField(blank=True, default="", max_length=32, db_index=True),
        ),
    ]
