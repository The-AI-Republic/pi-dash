# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Dispatch columns for the built-in engine as a first-class runner.

PIDESKAPP-123, ``.ai_design/built_engine_as_runner/design.md`` §7-§9:

- ``AgentRun.pin_strict``: the pin is the run's execution target (an issue's
  ``assigned_runner``) and is never released to the pod.
- ``Runner.pod_dispatch``: whether the runner takes unpinned pod work. Every
  existing row gets ``pod_member`` here, which is today's behaviour for
  own-agent runners; the data migration that follows sets engine runners to
  ``pinned_only``.
- ``DevMachine.engine_paused_by_owner`` / ``engine_auto_pause_reason`` /
  ``engine_team_runs``: per-machine engine controls.

Schema only and additive, so it reverses cleanly.
"""

from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0033_agent_run_error_details"),
    ]

    operations = [
        migrations.AddField(
            model_name="agentrun",
            name="pin_strict",
            field=models.BooleanField(default=False),
        ),
        migrations.AddField(
            model_name="runner",
            name="pod_dispatch",
            field=models.CharField(
                choices=[("pinned_only", "Pinned work only"), ("pod_member", "Takes pod work")],
                default="pod_member",
                max_length=16,
            ),
        ),
        migrations.AddField(
            model_name="devmachine",
            name="engine_paused_by_owner",
            field=models.BooleanField(default=False),
        ),
        migrations.AddField(
            model_name="devmachine",
            name="engine_auto_pause_reason",
            field=models.CharField(
                blank=True,
                choices=[
                    ("", "Not paused"),
                    ("engine_sign_in_required", "Engine sign-in required"),
                    ("gateway_session_revoked", "Gateway session revoked"),
                    ("wallet_empty", "Wallet empty"),
                    ("profile_unavailable", "Model profile unavailable"),
                    ("engine_disabled", "Engine disabled on this instance"),
                ],
                default="",
                max_length=32,
            ),
        ),
        migrations.AddField(
            model_name="devmachine",
            name="engine_team_runs",
            field=models.BooleanField(default=False),
        ),
        # Django drops the column default after backfilling. Keep a
        # database-level default too, so writers that do not go through the
        # Django model (the Rust API, raw SQL) can still insert rows without
        # naming the new NOT NULL columns.
        migrations.RunSQL(
            sql=[
                "ALTER TABLE agent_run ALTER COLUMN pin_strict SET DEFAULT false",
                "ALTER TABLE runner ALTER COLUMN pod_dispatch SET DEFAULT 'pod_member'",
                "ALTER TABLE dev_machine ALTER COLUMN engine_paused_by_owner SET DEFAULT false",
                "ALTER TABLE dev_machine ALTER COLUMN engine_auto_pause_reason SET DEFAULT ''",
                "ALTER TABLE dev_machine ALTER COLUMN engine_team_runs SET DEFAULT false",
            ],
            reverse_sql=[
                "ALTER TABLE agent_run ALTER COLUMN pin_strict DROP DEFAULT",
                "ALTER TABLE runner ALTER COLUMN pod_dispatch DROP DEFAULT",
                "ALTER TABLE dev_machine ALTER COLUMN engine_paused_by_owner DROP DEFAULT",
                "ALTER TABLE dev_machine ALTER COLUMN engine_auto_pause_reason DROP DEFAULT",
                "ALTER TABLE dev_machine ALTER COLUMN engine_team_runs DROP DEFAULT",
            ],
        ),
    ]
