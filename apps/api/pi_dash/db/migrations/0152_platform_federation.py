# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0151_github_pr_issue_link"),
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
    ]

    operations = [
        migrations.AddField(
            model_name="user",
            name="platform_user_id",
            field=models.UUIDField(blank=True, db_index=True, null=True, unique=True),
        ),
        migrations.AddField(
            model_name="user",
            name="platform_subject",
            field=models.CharField(blank=True, max_length=255, null=True, unique=True),
        ),
        migrations.AddField(
            model_name="user",
            name="platform_identity_linked_at",
            field=models.DateTimeField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="workspace",
            name="platform_org_id",
            field=models.UUIDField(blank=True, db_index=True, null=True, unique=True),
        ),
        migrations.AddField(
            model_name="workspace",
            name="platform_org_slug",
            field=models.CharField(blank=True, db_index=True, default="", max_length=128),
        ),
        migrations.AddField(
            model_name="workspace",
            name="platform_org_version",
            field=models.PositiveIntegerField(default=0),
        ),
        migrations.AddField(
            model_name="workspace",
            name="platform_linked_at",
            field=models.DateTimeField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="workspace",
            name="platform_access_disabled_at",
            field=models.DateTimeField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="workspacemember",
            name="platform_member_id",
            field=models.UUIDField(blank=True, db_index=True, null=True),
        ),
        migrations.AddField(
            model_name="workspacemember",
            name="platform_member_version",
            field=models.PositiveIntegerField(default=0),
        ),
        migrations.AddField(
            model_name="workspacemember",
            name="platform_member_status",
            field=models.CharField(blank=True, default="", max_length=32),
        ),
        migrations.AddField(
            model_name="workspacemember",
            name="platform_last_event_id",
            field=models.UUIDField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="workspacemember",
            name="platform_last_event_at",
            field=models.DateTimeField(blank=True, null=True),
        ),
        migrations.CreateModel(
            name="PlatformWebhookDelivery",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
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
                ("delivery_id", models.UUIDField(db_index=True, unique=True)),
                ("event_id", models.UUIDField(db_index=True, unique=True)),
                ("event_type", models.CharField(db_index=True, max_length=100)),
                ("platform_org_id", models.UUIDField(blank=True, db_index=True, null=True)),
                ("platform_user_id", models.UUIDField(blank=True, db_index=True, null=True)),
                ("payload", models.JSONField(default=dict)),
                (
                    "status",
                    models.CharField(
                        choices=[
                            ("received", "Received"),
                            ("processed", "Processed"),
                            ("failed", "Failed"),
                            ("skipped", "Skipped"),
                            ("dead_lettered", "Dead lettered"),
                        ],
                        default="received",
                        max_length=16,
                    ),
                ),
                ("attempt_count", models.PositiveIntegerField(default=0)),
                ("received_at", models.DateTimeField(auto_now_add=True)),
                ("processed_at", models.DateTimeField(blank=True, null=True)),
                ("error", models.TextField(blank=True, default="")),
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
            ],
            options={
                "verbose_name": "Platform Webhook Delivery",
                "verbose_name_plural": "Platform Webhook Deliveries",
                "db_table": "platform_webhook_deliveries",
                "ordering": ("-received_at",),
            },
        ),
        migrations.CreateModel(
            name="PlatformFederationState",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
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
                (
                    "status",
                    models.CharField(
                        choices=[("active", "Active"), ("disabled", "Disabled"), ("error", "Error")],
                        db_index=True,
                        default="active",
                        max_length=16,
                    ),
                ),
                ("last_event_id", models.UUIDField(blank=True, null=True)),
                ("last_reconciled_at", models.DateTimeField(blank=True, null=True)),
                ("last_error", models.TextField(blank=True, default="")),
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
                    "workspace",
                    models.OneToOneField(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="platform_federation_state",
                        to="db.workspace",
                    ),
                ),
            ],
            options={
                "verbose_name": "Platform Federation State",
                "verbose_name_plural": "Platform Federation States",
                "db_table": "platform_federation_states",
                "ordering": ("-updated_at",),
            },
        ),
        migrations.AddIndex(
            model_name="platformwebhookdelivery",
            index=models.Index(fields=["platform_org_id", "event_type"], name="platform_wh_org_event_idx"),
        ),
        migrations.AddIndex(
            model_name="platformwebhookdelivery",
            index=models.Index(fields=["platform_user_id", "event_type"], name="platform_wh_user_event_idx"),
        ),
        migrations.AddIndex(
            model_name="platformwebhookdelivery",
            index=models.Index(fields=["status", "received_at"], name="platform_wh_status_recv_idx"),
        ),
        migrations.AddIndex(
            model_name="workspacemember",
            index=models.Index(fields=["workspace", "platform_member_version"], name="workspace_member_platform_ver_idx"),
        ),
        migrations.AddConstraint(
            model_name="workspacemember",
            constraint=models.UniqueConstraint(
                condition=models.Q(("deleted_at__isnull", True), ("platform_member_id__isnull", False)),
                fields=("workspace", "platform_member_id"),
                name="workspace_member_unique_platform_member",
            ),
        ),
    ]
