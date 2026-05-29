# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
        ("runner", "0014_agent_run_usage"),
    ]

    operations = [
        migrations.CreateModel(
            name="DevMachine",
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
                ("host_label", models.CharField(blank=True, default="", max_length=255)),
                ("label", models.CharField(blank=True, default="", max_length=128)),
                (
                    "visibility",
                    models.PositiveSmallIntegerField(choices=[(0, "Private")], db_index=True, default=0),
                ),
                ("last_seen_at", models.DateTimeField(blank=True, null=True)),
                ("revoked_at", models.DateTimeField(blank=True, null=True)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("updated_at", models.DateTimeField(auto_now=True)),
                (
                    "owner",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="dev_machines",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
            ],
            options={
                "db_table": "dev_machine",
                "ordering": ("-last_seen_at", "-created_at"),
            },
        ),
        migrations.AddField(
            model_name="runner",
            name="visibility",
            field=models.PositiveSmallIntegerField(choices=[(0, "Private")], db_index=True, default=0),
        ),
        migrations.AddField(
            model_name="runner",
            name="dev_machine",
            field=models.ForeignKey(
                blank=True,
                null=True,
                on_delete=django.db.models.deletion.SET_NULL,
                related_name="runners",
                to="runner.devmachine",
            ),
        ),
        migrations.AddConstraint(
            model_name="devmachine",
            constraint=models.UniqueConstraint(
                condition=models.Q(("revoked_at__isnull", True)),
                fields=("owner", "host_label"),
                name="dev_machine_one_active_per_owner_host",
            ),
        ),
        migrations.AddIndex(
            model_name="devmachine",
            index=models.Index(fields=["owner", "visibility"], name="dev_machine_owner_vis_idx"),
        ),
        migrations.AddIndex(
            model_name="devmachine",
            index=models.Index(fields=["host_label"], name="dev_machine_host_idx"),
        ),
        migrations.AddIndex(
            model_name="runner",
            index=models.Index(
                fields=["dev_machine", "status"],
                name="runner_dev_machine_status_idx",
            ),
        ),
    ]
