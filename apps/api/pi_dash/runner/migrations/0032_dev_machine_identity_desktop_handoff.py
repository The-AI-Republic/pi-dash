# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""One record per dev machine + desktop sign-in handoff.

Adds ``DevMachine.fingerprint`` / ``merged_into`` / ``supervisor`` with a
partial unique constraint on (owner, fingerprint) for active, unmerged
records, and the hashed ``DesktopHandoffKey`` / ``DesktopHandoffCode``
tables. All new columns have defaults or are nullable, so existing rows
need no backfill.
"""

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
        ("runner", "0031_runner_paused"),
    ]

    operations = [
        migrations.AddField(
            model_name="devmachine",
            name="fingerprint",
            field=models.CharField(blank=True, default="", max_length=64),
        ),
        migrations.AddField(
            model_name="devmachine",
            name="merged_into",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="merged_from",
                to="runner.devmachine",
            ),
        ),
        migrations.AddField(
            model_name="devmachine",
            name="supervisor",
            field=models.CharField(blank=True, default="", max_length=16),
        ),
        migrations.AddConstraint(
            model_name="devmachine",
            constraint=models.UniqueConstraint(
                condition=models.Q(
                    ("merged_into__isnull", True),
                    ("revoked_at__isnull", True),
                    models.Q(("fingerprint", ""), _negated=True),
                ),
                fields=("owner", "fingerprint"),
                name="dev_machine_one_active_per_owner_fingerprint",
            ),
        ),
        migrations.CreateModel(
            name="DesktopHandoffKey",
            fields=[
                ("id", models.UUIDField(default=uuid.uuid4, editable=False, primary_key=True, serialize=False)),
                ("key_hash", models.CharField(db_index=True, max_length=128)),
                ("key_fingerprint", models.CharField(blank=True, default="", max_length=16)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("last_used_at", models.DateTimeField(blank=True, null=True)),
                ("revoked_at", models.DateTimeField(blank=True, null=True)),
                (
                    "dev_machine",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="handoff_keys",
                        to="runner.devmachine",
                    ),
                ),
                (
                    "user",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="desktop_handoff_keys",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
            ],
            options={
                "db_table": "desktop_handoff_key",
                "ordering": ("-created_at",),
                "indexes": [
                    models.Index(fields=["user", "dev_machine", "revoked_at"], name="desktop_handoff_key_um_idx")
                ],
            },
        ),
        migrations.CreateModel(
            name="DesktopHandoffCode",
            fields=[
                ("id", models.UUIDField(default=uuid.uuid4, editable=False, primary_key=True, serialize=False)),
                ("code_hash", models.CharField(max_length=128, unique=True)),
                ("expires_at", models.DateTimeField()),
                ("consumed_at", models.DateTimeField(blank=True, null=True)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                (
                    "key",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="codes",
                        to="runner.desktophandoffkey",
                    ),
                ),
                (
                    "user",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="desktop_handoff_codes",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
            ],
            options={
                "db_table": "desktop_handoff_code",
                "ordering": ("-created_at",),
                "indexes": [models.Index(fields=["expires_at"], name="desktop_handoff_code_exp_idx")],
            },
        ),
    ]
