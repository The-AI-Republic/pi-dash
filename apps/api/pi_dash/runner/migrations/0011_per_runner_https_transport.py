# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Drop the Connection trust unit; runner becomes the trust unit.

Implements the schema half of ``.ai_design/move_to_https/design.md``
Phase 1 (§6, §11). Single migration because there is no production
data to preserve (decision #13).

Steps:

1. Add trust/auth fields to ``Runner``.
2. Add ``RunnerSession`` and ``RunnerForceRefresh``.
3. Drop ``Runner.connection`` FK.
4. Drop the ``connection`` table.
5. Add ``MachineToken`` and ``RunMessageDedupe``.
"""

import uuid
import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("runner", "0010_agentrun_scheduler_binding"),
        ("db", "0132_project_scheduler_mvp"),
    ]

    operations = [
        # 1. Trust fields on Runner.
        migrations.AddField(
            model_name="runner",
            name="host_label",
            field=models.CharField(blank=True, default="", max_length=255),
        ),
        migrations.AddField(
            model_name="runner",
            name="refresh_token_hash",
            field=models.CharField(
                blank=True, db_index=True, default="", max_length=128
            ),
        ),
        migrations.AddField(
            model_name="runner",
            name="refresh_token_fingerprint",
            field=models.CharField(blank=True, default="", max_length=16),
        ),
        migrations.AddField(
            model_name="runner",
            name="refresh_token_generation",
            field=models.PositiveIntegerField(default=0),
        ),
        migrations.AddField(
            model_name="runner",
            name="previous_refresh_token_hash",
            field=models.CharField(blank=True, default="", max_length=128),
        ),
        migrations.AddField(
            model_name="runner",
            name="access_token_signing_key_version",
            field=models.PositiveIntegerField(default=1),
        ),
        migrations.AddField(
            model_name="runner",
            name="enrollment_token_hash",
            field=models.CharField(blank=True, default="", max_length=128),
        ),
        migrations.AddField(
            model_name="runner",
            name="enrollment_token_fingerprint",
            field=models.CharField(blank=True, default="", max_length=16),
        ),
        migrations.AddField(
            model_name="runner",
            name="enrolled_at",
            field=models.DateTimeField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="runner",
            name="revoked_reason",
            field=models.CharField(blank=True, default="", max_length=32),
        ),

        # 2. New session/force-refresh tables (created first so their
        #    runner FK can point at Runner; no Runner.connection FK is
        #    involved).
        migrations.CreateModel(
            name="RunnerSession",
            fields=[
                (
                    "id",
                    models.UUIDField(
                        default=uuid.uuid4,
                        editable=False,
                        primary_key=True,
                        serialize=False,
                    ),
                ),
                ("protocol_version", models.PositiveIntegerField(default=4)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("last_seen_at", models.DateTimeField(blank=True, null=True)),
                ("revoked_at", models.DateTimeField(blank=True, null=True)),
                (
                    "revoked_reason",
                    models.CharField(blank=True, default="", max_length=32),
                ),
                (
                    "runner",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="sessions",
                        to="runner.runner",
                    ),
                ),
            ],
            options={
                "db_table": "runner_session",
                "ordering": ("-created_at",),
            },
        ),
        migrations.AddConstraint(
            model_name="runnersession",
            constraint=models.UniqueConstraint(
                condition=models.Q(("revoked_at__isnull", True)),
                fields=("runner",),
                name="runner_session_one_active_per_runner",
            ),
        ),
        migrations.AddIndex(
            model_name="runnersession",
            index=models.Index(
                fields=["runner", "revoked_at"],
                name="runner_sess_runner__idx",
            ),
        ),
        migrations.AddIndex(
            model_name="runnersession",
            index=models.Index(
                fields=["last_seen_at"], name="runner_sess_last_se_idx"
            ),
        ),
        migrations.CreateModel(
            name="RunnerForceRefresh",
            fields=[
                (
                    "runner",
                    models.OneToOneField(
                        on_delete=django.db.models.deletion.CASCADE,
                        primary_key=True,
                        related_name="force_refresh",
                        serialize=False,
                        to="runner.runner",
                    ),
                ),
                ("min_rtg", models.PositiveIntegerField(default=0)),
                ("reason", models.CharField(blank=True, default="", max_length=64)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
            ],
            options={"db_table": "runner_force_refresh"},
        ),

        # 3-4. Drop the Connection FK + table.
        migrations.RemoveField(model_name="runner", name="connection"),
        migrations.DeleteModel(name="Connection"),

        # 5. MachineToken + RunMessageDedupe.
        migrations.CreateModel(
            name="MachineToken",
            fields=[
                (
                    "id",
                    models.UUIDField(
                        default=uuid.uuid4,
                        editable=False,
                        primary_key=True,
                        serialize=False,
                    ),
                ),
                ("host_label", models.CharField(max_length=255)),
                (
                    "token_hash",
                    models.CharField(db_index=True, max_length=128),
                ),
                (
                    "token_fingerprint",
                    models.CharField(blank=True, default="", max_length=16),
                ),
                (
                    "label",
                    models.CharField(blank=True, default="", max_length=128),
                ),
                ("is_service", models.BooleanField(default=True)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("last_used_at", models.DateTimeField(blank=True, null=True)),
                ("revoked_at", models.DateTimeField(blank=True, null=True)),
                (
                    "user",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="machine_tokens",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "workspace",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="machine_tokens",
                        to="db.workspace",
                    ),
                ),
            ],
            options={
                "db_table": "machine_token",
                "ordering": ("-created_at",),
            },
        ),
        migrations.AddConstraint(
            model_name="machinetoken",
            constraint=models.UniqueConstraint(
                condition=models.Q(("revoked_at__isnull", True)),
                fields=("user", "workspace", "host_label"),
                name="machine_token_one_active_per_user_ws_host",
            ),
        ),
        migrations.AddIndex(
            model_name="machinetoken",
            index=models.Index(
                fields=["user", "workspace", "revoked_at"],
                name="machine_tok_user_id_idx",
            ),
        ),
        migrations.CreateModel(
            name="RunMessageDedupe",
            fields=[
                (
                    "id",
                    models.BigAutoField(
                        auto_created=True,
                        primary_key=True,
                        serialize=False,
                    ),
                ),
                ("message_id", models.CharField(max_length=128)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                (
                    "run",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="message_dedupes",
                        to="runner.agentrun",
                    ),
                ),
            ],
            options={"db_table": "run_message_dedupe"},
        ),
        migrations.AddConstraint(
            model_name="runmessagededupe",
            constraint=models.UniqueConstraint(
                fields=("run", "message_id"),
                name="run_message_dedupe_unique",
            ),
        ),
        migrations.AddIndex(
            model_name="runmessagededupe",
            index=models.Index(
                fields=["created_at"], name="run_msg_dedu_created_idx"
            ),
        ),
    ]
