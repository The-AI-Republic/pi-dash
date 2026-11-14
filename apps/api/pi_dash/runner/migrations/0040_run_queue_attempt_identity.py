# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""AgentRun/run-queue attempt identity + RunnerSession capability (runner 0040).

Dev-machine AgentRun queue T1 (``.ai_design/dev_machine_run_queue/``,
implementation.md §3). Additive and nullable: existing rows stay
legacy (protocol 0, no token, no capacity_wait, no session
capabilities) with no backfill. ``assignment_token`` is minted inside
every new assignment transaction and preserved on redelivery;
``assignment_protocol`` is immutable per attempt; ``capacity_wait``
carries the daemon-reported capacity phase; ``RunnerSession``
``capabilities`` records the daemon's negotiated
``machine_run_queue_v1`` advertisement.

No index on ``assignment_token``: lookups are by run id (the attempt
is already pinned by the run row), and the spec adds one only if an
existing query shape needs it.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0039_issue_assigned_runner"),
    ]

    operations = [
        migrations.AddField(
            model_name="agentrun",
            name="assignment_token",
            field=models.UUIDField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="agentrun",
            name="assignment_protocol",
            field=models.PositiveSmallIntegerField(
                choices=[(0, "Legacy"), (1, "Machine queue")],
                default=0,
            ),
        ),
        migrations.AddField(
            model_name="agentrun",
            name="capacity_wait",
            field=models.JSONField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="runnersession",
            name="capabilities",
            field=models.JSONField(blank=True, default=list),
        ),
    ]
