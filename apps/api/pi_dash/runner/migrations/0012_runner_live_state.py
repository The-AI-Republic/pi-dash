# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add RunnerLiveState — per-runner volatile observability snapshot.

See ``.ai_design/runner_agent_bridge/design.md`` §4.5.1. This is an
additive migration: a new table with a one-to-one FK to ``Runner`` and a
covering index for the stall-watchdog query. No existing tables are
modified, no data migration is required, and rolling back is safe (the
table can be left in place during a temporary rollback).
"""

import django.db.models.deletion
from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("runner", "0011_per_runner_https_transport"),
    ]

    operations = [
        migrations.CreateModel(
            name="RunnerLiveState",
            fields=[
                (
                    "runner",
                    models.OneToOneField(
                        on_delete=django.db.models.deletion.CASCADE,
                        primary_key=True,
                        related_name="live_state",
                        serialize=False,
                        to="runner.runner",
                    ),
                ),
                ("observed_run_id", models.UUIDField(blank=True, null=True)),
                ("last_event_at", models.DateTimeField(blank=True, null=True)),
                (
                    "last_event_kind",
                    models.CharField(blank=True, max_length=64, null=True),
                ),
                (
                    "last_event_summary",
                    models.CharField(blank=True, max_length=200, null=True),
                ),
                ("agent_pid", models.PositiveIntegerField(blank=True, null=True)),
                ("agent_subprocess_alive", models.BooleanField(blank=True, null=True)),
                (
                    "approvals_pending",
                    models.PositiveSmallIntegerField(blank=True, null=True),
                ),
                ("input_tokens", models.BigIntegerField(blank=True, null=True)),
                ("output_tokens", models.BigIntegerField(blank=True, null=True)),
                ("total_tokens", models.BigIntegerField(blank=True, null=True)),
                ("turn_count", models.PositiveIntegerField(blank=True, null=True)),
                ("updated_at", models.DateTimeField(auto_now=True)),
            ],
            options={
                "db_table": "runner_live_state",
            },
        ),
        migrations.AddIndex(
            model_name="runnerlivestate",
            index=models.Index(
                fields=["observed_run_id", "updated_at", "last_event_at"],
                name="runner_live_watchdog_idx",
            ),
        ),
    ]
