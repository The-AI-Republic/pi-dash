# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Desktop-bundled managed runner: provisioning columns and a widened
``AgentRun`` executor constraint.

``agent_run_cloud_has_no_local_assignment`` previously admitted only
``local_runner`` **or** a ``cloud_agent`` row with every local-assignment
column NULL. A ``managed_runner`` row satisfies neither branch, so inserting
one raises ``IntegrityError`` — this migration must land before any code path
can create managed work. See ``.ai_design/managed_runner/design.md`` §7.3.

The replacement keeps the cloud branch byte-for-byte and only widens the
machine-executor branch to the ``MACHINE_EXECUTORS`` tuple, so existing rows
are unaffected and the migration reverses cleanly.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0023_one_active_includes_cancel_requested"),
    ]

    operations = [
        migrations.AddField(
            model_name="devmachine",
            name="provisioning",
            field=models.CharField(
                choices=[
                    ("manual", "Enrolled by the user"),
                    ("desktop_bundled", "Provisioned by Pi Dash Desktop"),
                ],
                db_index=True,
                default="manual",
                max_length=24,
            ),
        ),
        migrations.AddField(
            model_name="runner",
            name="provisioning",
            field=models.CharField(
                choices=[
                    ("manual", "Enrolled by the user"),
                    ("desktop_bundled", "Provisioned by Pi Dash Desktop"),
                ],
                db_index=True,
                default="manual",
                max_length=24,
            ),
        ),
        migrations.RemoveConstraint(
            model_name="agentrun",
            name="agent_run_cloud_has_no_local_assignment",
        ),
        migrations.AddConstraint(
            model_name="agentrun",
            constraint=models.CheckConstraint(
                condition=models.Q(("executor_kind__in", ["local_runner", "managed_runner"]))
                | models.Q(
                    ("executor_kind", "cloud_agent"),
                    ("runner__isnull", True),
                    ("pinned_runner__isnull", True),
                    ("owner__isnull", True),
                    ("assigned_at__isnull", True),
                    ("queue_position__isnull", True),
                ),
                name="agent_run_cloud_has_no_local_assignment",
            ),
        ),
    ]
