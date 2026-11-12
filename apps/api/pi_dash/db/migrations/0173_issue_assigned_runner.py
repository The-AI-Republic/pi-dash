# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Issue.assigned_runner, Project.dedicated_runner_lost_action (db 0173).

Dedicated-runner design §6.5/#6.6/§6.2 (I1). Nullable columns and a
constant default: metadata-only on Postgres 11+. The partial index is
built concurrently so production does not take an ACCESS EXCLUSIVE lock
on a populated ``issues`` table (repo pattern at 0142). ``atomic = False``
is required because CREATE INDEX CONCURRENTLY cannot run in a transaction.

Pruned from the autodetector's output: it also carried unrelated drift on
main (audit-FK verbose_name, executor choices, ticker options, scheduler
fields); those belong to their own migrations, not this one.
"""

from django.db import migrations, models
import django.db.models.deletion
from django.contrib.postgres.operations import AddIndexConcurrently


class Migration(migrations.Migration):
    atomic = False

    dependencies = [
        ("runner", "0039_issue_assigned_runner"),
        ("db", "0172_clidevicecode_engine_scope"),
    ]

    operations = [
        migrations.AddField(
            model_name="issue",
            name="assigned_runner",
            field=models.ForeignKey(
                blank=True,
                db_index=False,
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="dedicated_issues",
                to="runner.runner",
            ),
        ),
        migrations.AddField(
            model_name="project",
            name="dedicated_runner_lost_action",
            field=models.CharField(
                choices=[
                    ("fail_to_backlog", "Fail the run and move the issue to Backlog"),
                    ("fall_back", "Fall back to the pod"),
                ],
                default="fail_to_backlog",
                max_length=24,
            ),
        ),
        AddIndexConcurrently(
            model_name="issue",
            index=models.Index(
                condition=models.Q(("assigned_runner__isnull", False)),
                fields=["assigned_runner"],
                name="issues_assigned_runner_idx",
            ),
        ),
        migrations.AddConstraint(
            model_name="issue",
            constraint=models.CheckConstraint(
                check=models.Q(
                    ("assigned_runner__isnull", True),
                    ("assigned_pod__isnull", False),
                    _connector="OR",
                ),
                name="issues_dedicated_runner_needs_pod",
            ),
        ),
    ]
