# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.conf import settings
from django.db import migrations, models
import django.db.models.deletion
import uuid


class Migration(migrations.Migration):
    initial = True

    dependencies = [
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
        ("db", "0001_initial"),
    ]

    operations = [
        migrations.CreateModel(
            name="Runner",
            fields=[
                (
                    "id",
                    models.UUIDField(
                        db_index=True,
                        default=uuid.uuid4,
                        editable=False,
                        primary_key=True,
                        serialize=False,
                    ),
                ),
                ("name", models.CharField(max_length=128)),
                ("credential_hash", models.CharField(db_index=True, max_length=128)),
                ("credential_fingerprint", models.CharField(max_length=16)),
                ("capabilities", models.JSONField(blank=True, default=list)),
                (
                    "status",
                    models.CharField(
                        choices=[
                            ("online", "Online"),
                            ("offline", "Offline"),
                            ("busy", "Busy"),
                            ("revoked", "Revoked"),
                        ],
                        db_index=True,
                        default="offline",
                        max_length=16,
                    ),
                ),
                ("os", models.CharField(blank=True, default="", max_length=32)),
                ("arch", models.CharField(blank=True, default="", max_length=32)),
                (
                    "runner_version",
                    models.CharField(blank=True, default="", max_length=32),
                ),
                ("protocol_version", models.PositiveIntegerField(default=1)),
                ("last_heartbeat_at", models.DateTimeField(blank=True, null=True)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("updated_at", models.DateTimeField(auto_now=True)),
                ("revoked_at", models.DateTimeField(blank=True, null=True)),
                (
                    "owner",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="runners",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "workspace",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="runners",
                        to="db.workspace",
                    ),
                ),
            ],
            options={
                "db_table": "runner",
                "ordering": ("-last_heartbeat_at", "-created_at"),
            },
        ),
        migrations.AddIndex(
            model_name="runner",
            index=models.Index(
                fields=["owner", "status"], name="runner_owner_status_idx"
            ),
        ),
        migrations.AddIndex(
            model_name="runner",
            index=models.Index(
                fields=["workspace", "status"], name="runner_ws_status_idx"
            ),
        ),
        migrations.CreateModel(
            name="RunnerRegistrationToken",
            fields=[
                (
                    "id",
                    models.UUIDField(
                        db_index=True,
                        default=uuid.uuid4,
                        editable=False,
                        primary_key=True,
                        serialize=False,
                    ),
                ),
                ("token_hash", models.CharField(max_length=128, unique=True)),
                ("label", models.CharField(blank=True, default="", max_length=128)),
                ("expires_at", models.DateTimeField()),
                ("consumed_at", models.DateTimeField(blank=True, null=True)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                (
                    "created_by",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="runner_registration_tokens",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "workspace",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="runner_registration_tokens",
                        to="db.workspace",
                    ),
                ),
                (
                    "consumed_by_runner",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="consumed_registration_tokens",
                        to="runner.runner",
                    ),
                ),
            ],
            options={
                "db_table": "runner_registration_token",
                "ordering": ("-created_at",),
            },
        ),
        migrations.AddIndex(
            model_name="runnerregistrationtoken",
            index=models.Index(
                fields=["workspace", "consumed_at"], name="runnreg_ws_consumed_idx"
            ),
        ),
        migrations.CreateModel(
            name="AgentRun",
            fields=[
                (
                    "id",
                    models.UUIDField(
                        db_index=True,
                        default=uuid.uuid4,
                        editable=False,
                        primary_key=True,
                        serialize=False,
                    ),
                ),
                (
                    "status",
                    models.CharField(
                        choices=[
                            ("queued", "Queued"),
                            ("assigned", "Assigned"),
                            ("running", "Running"),
                            ("awaiting_approval", "Awaiting Approval"),
                            ("awaiting_reauth", "Awaiting Reauth"),
                            ("completed", "Completed"),
                            ("failed", "Failed"),
                            ("cancelled", "Cancelled"),
                        ],
                        db_index=True,
                        default="queued",
                        max_length=24,
                    ),
                ),
                ("prompt", models.TextField(blank=True, default="")),
                ("run_config", models.JSONField(blank=True, default=dict)),
                ("required_capabilities", models.JSONField(blank=True, default=list)),
                ("thread_id", models.CharField(blank=True, default="", max_length=128)),
                ("lease_expires_at", models.DateTimeField(blank=True, null=True)),
                ("done_payload", models.JSONField(blank=True, null=True)),
                ("error", models.TextField(blank=True, default="")),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("assigned_at", models.DateTimeField(blank=True, null=True)),
                ("started_at", models.DateTimeField(blank=True, null=True)),
                ("ended_at", models.DateTimeField(blank=True, null=True)),
                (
                    "owner",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="agent_runs",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "workspace",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="agent_runs",
                        to="db.workspace",
                    ),
                ),
                (
                    "runner",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="agent_runs",
                        to="runner.runner",
                    ),
                ),
                (
                    "work_item",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="agent_runs",
                        to="db.issue",
                    ),
                ),
            ],
            options={
                "db_table": "agent_run",
                "ordering": ("-created_at",),
            },
        ),
        migrations.AddIndex(
            model_name="agentrun",
            index=models.Index(
                fields=["runner", "status"], name="agentrun_runner_status_idx"
            ),
        ),
        migrations.AddIndex(
            model_name="agentrun",
            index=models.Index(
                fields=["owner", "status"], name="agentrun_owner_status_idx"
            ),
        ),
        migrations.AddIndex(
            model_name="agentrun",
            index=models.Index(
                fields=["workspace", "status"], name="agentrun_ws_status_idx"
            ),
        ),
        migrations.CreateModel(
            name="AgentRunEvent",
            fields=[
                ("id", models.BigAutoField(primary_key=True, serialize=False)),
                ("seq", models.PositiveIntegerField()),
                ("kind", models.CharField(max_length=64)),
                ("payload", models.JSONField(blank=True, default=dict)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                (
                    "agent_run",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="events",
                        to="runner.agentrun",
                    ),
                ),
            ],
            options={
                "db_table": "agent_run_event",
                "ordering": ("agent_run", "seq"),
                "unique_together": {("agent_run", "seq")},
            },
        ),
        migrations.CreateModel(
            name="ApprovalRequest",
            fields=[
                (
                    "id",
                    models.UUIDField(
                        db_index=True,
                        default=uuid.uuid4,
                        editable=False,
                        primary_key=True,
                        serialize=False,
                    ),
                ),
                (
                    "kind",
                    models.CharField(
                        choices=[
                            ("command_execution", "Command Execution"),
                            ("file_change", "File Change"),
                            ("network_access", "Network Access"),
                            ("other", "Other"),
                        ],
                        max_length=24,
                    ),
                ),
                ("payload", models.JSONField(blank=True, default=dict)),
                ("reason", models.TextField(blank=True, default="")),
                (
                    "status",
                    models.CharField(
                        choices=[
                            ("pending", "Pending"),
                            ("accepted", "Accepted"),
                            ("declined", "Declined"),
                            ("expired", "Expired"),
                        ],
                        db_index=True,
                        default="pending",
                        max_length=16,
                    ),
                ),
                (
                    "decision_source",
                    models.CharField(blank=True, default="", max_length=16),
                ),
                ("requested_at", models.DateTimeField(auto_now_add=True)),
                ("expires_at", models.DateTimeField(blank=True, null=True)),
                ("decided_at", models.DateTimeField(blank=True, null=True)),
                (
                    "agent_run",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="approvals",
                        to="runner.agentrun",
                    ),
                ),
                (
                    "decided_by",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="runner_approvals_decided",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
            ],
            options={
                "db_table": "agent_run_approval",
                "ordering": ("-requested_at",),
            },
        ),
        migrations.AddIndex(
            model_name="approvalrequest",
            index=models.Index(
                fields=["agent_run", "status"], name="approval_run_status_idx"
            ),
        ),
    ]
