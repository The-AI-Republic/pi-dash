# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""AgentRun.pin_kind (runner 0039).

Dedicated-runner design §6.4/§6.5/#6.2 (I5). Additive with a default:
existing rows stay ``locality`` with no backfill. The last-served index is
built concurrently (design §6.5, for the matcher's alternation lookup
§10.2); ``atomic = False`` is required for CREATE INDEX CONCURRENTLY.

Pruned from the autodetector's output: it also carried unrelated drift on
main (executor choices, index renames, RunMessageDedupe id, the cloud
constraint remove/re-add); those belong to their own migrations.
"""

from django.db import migrations, models
from django.contrib.postgres.operations import AddIndexConcurrently


class Migration(migrations.Migration):
    atomic = False

    dependencies = [
        ("runner", "0038_rename_engine_runners"),
    ]

    operations = [
        migrations.AddField(
            model_name="agentrun",
            name="pin_kind",
            field=models.CharField(
                choices=[
                    ("locality", "Locality hint"),
                    ("dedicated", "Dedicated runner"),
                ],
                default="locality",
                max_length=16,
            ),
        ),
        AddIndexConcurrently(
            model_name="agentrun",
            index=models.Index(
                fields=["runner", "-assigned_at"], name="agent_run_runner_assigned_idx"
            ),
        ),
        migrations.AddConstraint(
            model_name="agentrun",
            constraint=models.CheckConstraint(
                check=models.Q(
                    ("pin_kind", "locality"),
                    ("executor_kind", "local_runner"),
                    _connector="OR",
                ),
                name="agent_run_dedicated_pin_is_local",
            ),
        ),
    ]
