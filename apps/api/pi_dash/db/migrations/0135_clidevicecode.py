# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Add CLIDeviceCode model for `pidash auth login` device-code flow.

RFC 8628-shaped grant: a row is created when the CLI starts a login,
approved by the user via the web UI, and consumed when the CLI trades
the device_code for a fresh APIToken.
"""

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models

import pi_dash.db.models.api


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0134_issueagentticker_disarm_reason"),
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
    ]

    operations = [
        migrations.CreateModel(
            name="CLIDeviceCode",
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
                    "device_code",
                    models.CharField(
                        db_index=True,
                        default=pi_dash.db.models.api.generate_device_code,
                        max_length=64,
                        unique=True,
                    ),
                ),
                (
                    "user_code",
                    models.CharField(
                        db_index=True,
                        default=pi_dash.db.models.api.generate_user_code,
                        max_length=16,
                        unique=True,
                    ),
                ),
                ("approved", models.BooleanField(default=False)),
                ("denied", models.BooleanField(default=False)),
                ("consumed", models.BooleanField(default=False)),
                ("expires_at", models.DateTimeField()),
                ("last_polled_at", models.DateTimeField(blank=True, null=True)),
                (
                    "created_by",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="clidevicecode_created_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Created By",
                    ),
                ),
                (
                    "updated_by",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="clidevicecode_updated_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Last Modified By",
                    ),
                ),
                (
                    "user",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="device_codes",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "workspace",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="device_codes",
                        to="db.workspace",
                    ),
                ),
            ],
            options={
                "verbose_name": "CLI Device Code",
                "verbose_name_plural": "CLI Device Codes",
                "db_table": "cli_device_codes",
                "ordering": ("-created_at",),
            },
        ),
    ]
