# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import uuid

import django.db.models.deletion
from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ("runner", "0018_agentrun_trigger_manifest"),
    ]

    operations = [
        migrations.CreateModel(
            name="MachineSession",
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
                    "dev_machine",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="sessions",
                        to="runner.devmachine",
                    ),
                ),
            ],
            options={
                "db_table": "machine_session",
                "ordering": ("-created_at",),
            },
        ),
        migrations.AddConstraint(
            model_name="machinesession",
            constraint=models.UniqueConstraint(
                condition=models.Q(("revoked_at__isnull", True)),
                fields=("dev_machine",),
                name="machine_session_one_active_per_machine",
            ),
        ),
        migrations.AddIndex(
            model_name="machinesession",
            index=models.Index(
                fields=["dev_machine", "revoked_at"],
                name="machine_ses_dev_mac_885f2b_idx",
            ),
        ),
        migrations.AddIndex(
            model_name="machinesession",
            index=models.Index(
                fields=["last_seen_at"], name="machine_ses_last_se_b57ca0_idx"
            ),
        ),
    ]
