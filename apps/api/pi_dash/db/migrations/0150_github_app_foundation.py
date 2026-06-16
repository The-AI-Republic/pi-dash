# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0149_loop_mvp"),
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
    ]

    operations = [
        migrations.CreateModel(
            name="GithubWebhookDelivery",
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
                ("event", models.CharField(max_length=100)),
                ("action", models.CharField(blank=True, default="", max_length=100)),
                ("installation_id", models.BigIntegerField(blank=True, db_index=True, null=True)),
                ("payload", models.JSONField(default=dict)),
                (
                    "status",
                    models.CharField(
                        choices=[
                            ("received", "Received"),
                            ("processed", "Processed"),
                            ("failed", "Failed"),
                            ("skipped", "Skipped"),
                        ],
                        default="received",
                        max_length=16,
                    ),
                ),
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
                "verbose_name": "Github Webhook Delivery",
                "verbose_name_plural": "Github Webhook Deliveries",
                "db_table": "github_webhook_deliveries",
                "ordering": ("-received_at",),
            },
        ),
        migrations.CreateModel(
            name="GithubAppInstallation",
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
                ("installation_id", models.BigIntegerField(db_index=True, unique=True)),
                ("account_login", models.CharField(blank=True, default="", max_length=255)),
                (
                    "account_type",
                    models.CharField(
                        choices=[("User", "User"), ("Organization", "Organization"), ("Unknown", "Unknown")],
                        default="Unknown",
                        max_length=32,
                    ),
                ),
                (
                    "repository_selection",
                    models.CharField(choices=[("all", "All"), ("selected", "Selected")], default="selected", max_length=16),
                ),
                ("repository_count", models.PositiveIntegerField(default=0)),
                ("permissions", models.JSONField(default=dict)),
                ("events", models.JSONField(default=list)),
                ("installed_at", models.DateTimeField(blank=True, null=True)),
                ("suspended_at", models.DateTimeField(blank=True, null=True)),
                ("verified_at", models.DateTimeField(blank=True, null=True)),
                ("last_checked_at", models.DateTimeField(blank=True, null=True)),
                ("last_check_error", models.TextField(blank=True, default="")),
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
                    "workspace_integration",
                    models.OneToOneField(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="github_app_installation",
                        to="db.workspaceintegration",
                    ),
                ),
            ],
            options={
                "verbose_name": "Github App Installation",
                "verbose_name_plural": "Github App Installations",
                "db_table": "github_app_installations",
                "ordering": ("-created_at",),
            },
        ),
        migrations.CreateModel(
            name="GithubAppInstallSession",
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
                ("state", models.CharField(db_index=True, max_length=128, unique=True)),
                ("installation_id", models.BigIntegerField(blank=True, null=True)),
                ("account_login", models.CharField(blank=True, default="", max_length=255)),
                (
                    "status",
                    models.CharField(
                        choices=[
                            ("started", "Started"),
                            ("completed", "Completed"),
                            ("expired", "Expired"),
                            ("failed", "Failed"),
                        ],
                        db_index=True,
                        default="started",
                        max_length=16,
                    ),
                ),
                ("expires_at", models.DateTimeField(db_index=True)),
                ("completed_at", models.DateTimeField(blank=True, null=True)),
                ("error", models.TextField(blank=True, default="")),
                (
                    "actor",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="github_app_install_sessions",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
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
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="github_app_install_sessions",
                        to="db.workspace",
                    ),
                ),
            ],
            options={
                "verbose_name": "Github App Install Session",
                "verbose_name_plural": "Github App Install Sessions",
                "db_table": "github_app_install_sessions",
                "ordering": ("-created_at",),
            },
        ),
    ]
