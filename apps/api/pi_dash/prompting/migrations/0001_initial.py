# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):
    initial = True

    dependencies = [
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
        ("db", "0001_initial"),
    ]

    operations = [
        migrations.CreateModel(
            name="PromptTemplate",
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
                ("name", models.CharField(default="coding-task", max_length=64)),
                ("body", models.TextField()),
                ("is_active", models.BooleanField(default=True)),
                ("version", models.PositiveIntegerField(default=1)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("updated_at", models.DateTimeField(auto_now=True)),
                (
                    "updated_by",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="prompt_templates_updated",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "workspace",
                    models.ForeignKey(
                        blank=True,
                        help_text="NULL = global default template.",
                        null=True,
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="prompt_templates",
                        to="db.workspace",
                    ),
                ),
            ],
            options={
                "db_table": "prompt_template",
            },
        ),
        migrations.AddIndex(
            model_name="prompttemplate",
            index=models.Index(
                fields=["workspace", "name", "is_active"],
                name="prompt_temp_workspa_e56e29_idx",
            ),
        ),
        migrations.AddConstraint(
            model_name="prompttemplate",
            constraint=models.UniqueConstraint(
                condition=models.Q(("is_active", True)),
                fields=("workspace", "name"),
                name="prompt_template_one_active_per_ws_name",
            ),
        ),
    ]
