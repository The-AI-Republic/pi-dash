# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Persist the dev-machine concurrent-run limit (NEWPIDASH-398).

Adds ``DevMachine.run_concurrency_limit``: the last concurrent-issue-run
limit the daemon reported, written only from the validated ``limit`` in
capacity reports and poll snapshots. NULL means the machine never
reported one, and the matcher assigns as it did before.

Schema-only: no Python behaviour. Nullable, so older writers that omit
the column keep working.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0044_agentrun_queue_columns"),
    ]

    operations = [
        migrations.AddField(
            model_name="devmachine",
            name="run_concurrency_limit",
            field=models.IntegerField(blank=True, null=True),
        ),
    ]
