# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Schema for the periodic agent ticking system.

Adds:

- ``IssueAgentSchedule`` model — one row per issue, storing the per-issue
  clock that drives periodic agent re-invocation.
- Three project fields: ``agent_default_interval_seconds``,
  ``agent_default_max_ticks``, ``agent_ticking_enabled``.

See ``.ai_design/issue_ticking_system/design.md`` §7.
"""

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0126_issue_assigned_pod"),
    ]

    operations = [
        migrations.AddField(
            model_name="project",
            name="agent_default_interval_seconds",
            field=models.IntegerField(default=10800),
        ),
        migrations.AddField(
            model_name="project",
            name="agent_default_max_ticks",
            field=models.IntegerField(default=24),
        ),
        migrations.AddField(
            model_name="project",
            name="agent_ticking_enabled",
            field=models.BooleanField(default=True),
        ),
        migrations.CreateModel(
            name="IssueAgentSchedule",
            fields=[
                (
                    "created_at",
                    models.DateTimeField(auto_now_add=True, verbose_name="Created At"),
                ),
                (
                    "updated_at",
                    models.DateTimeField(auto_now=True, verbose_name="Last Modified At"),
                ),
                (
                    "deleted_at",
                    models.DateTimeField(
                        blank=True, null=True, verbose_name="Deleted At"
                    ),
                ),
                (
                    "id",
                    models.UUIDField(
                        db_index=True,
                        default=uuid.uuid4,
                        editable=False,
                        primary_key=True,
                        serialize=False,
                        unique=True,
                    ),
                ),
                ("interval_seconds", models.IntegerField(blank=True, null=True)),
                ("max_ticks", models.IntegerField(blank=True, null=True)),
                ("user_disabled", models.BooleanField(default=False)),
                ("next_run_at", models.DateTimeField(blank=True, null=True)),
                ("tick_count", models.IntegerField(default=0)),
                ("last_tick_at", models.DateTimeField(blank=True, null=True)),
                ("enabled", models.BooleanField(default=True)),
                (
                    "created_by",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="%(class)s_created_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Created By",
                    ),
                ),
                (
                    "updated_by",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="%(class)s_updated_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Last Modified By",
                    ),
                ),
                (
                    "issue",
                    models.OneToOneField(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="agent_schedule",
                        to="db.issue",
                    ),
                ),
            ],
            options={
                "verbose_name": "Issue Agent Schedule",
                "verbose_name_plural": "Issue Agent Schedules",
                "db_table": "issue_agent_schedule",
                "indexes": [
                    models.Index(
                        fields=["enabled", "next_run_at"],
                        name="iasched_enabled_next_run_idx",
                    ),
                ],
            },
        ),
    ]
