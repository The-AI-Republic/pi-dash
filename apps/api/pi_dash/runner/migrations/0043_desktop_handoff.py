# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Desktop session handoff code (desktop_unified_signin §5).

A single-use code that turns a daemon credential into a desktop session:
random, stored hashed, bound to the user, the dev machine and the app's
challenge, valid 60 seconds. Redeem consumes it on the first attempt
whatever the outcome.
"""

import uuid

import django.db.models.deletion
from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        ("runner", "0042_agentrun_work_type"),
    ]

    operations = [
        migrations.CreateModel(
            name="DesktopHandoffCode",
            fields=[
                ("id", models.UUIDField(default=uuid.uuid4, editable=False, primary_key=True, serialize=False)),
                ("code_hash", models.CharField(db_index=True, max_length=128, unique=True)),
                ("fingerprint", models.CharField(blank=True, default="", max_length=16)),
                ("challenge", models.CharField(max_length=128)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("expires_at", models.DateTimeField()),
                ("consumed_at", models.DateTimeField(blank=True, null=True)),
                (
                    "dev_machine",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="desktop_handoff_codes",
                        to="runner.devmachine",
                    ),
                ),
                (
                    "user",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="desktop_handoff_codes",
                        to="db.user",
                    ),
                ),
            ],
            options={
                "db_table": "desktop_handoff_code",
                "ordering": ("-created_at",),
                "indexes": [
                    models.Index(fields=["user", "dev_machine", "created_at"], name="handoff_user_machine_idx")
                ],
            },
        )
    ]
