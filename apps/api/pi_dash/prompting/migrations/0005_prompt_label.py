# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Workspace-scoped prompt labels for sections and receipts (PDASHOSS01-71).

Introduces ``PromptLabel`` (a workspace-shared, user-managed label) and
``PromptLabelAssignment`` (the n:n attachment of a label to a code-defined
prompt section — keyed by ``section_key`` — or receipt — keyed by prompt
``kind``). Kept separate from the issue ``Label`` model, which is
project/issue-coupled and hierarchical.
"""

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


class Migration(migrations.Migration):
    dependencies = [
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
        ("db", "0001_initial"),
        ("prompting", "0004_prompt_section_override"),
    ]

    operations = [
        migrations.CreateModel(
            name="PromptLabel",
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
                ("name", models.CharField(max_length=64)),
                ("color", models.CharField(default="#6b7280", max_length=32)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("updated_at", models.DateTimeField(auto_now=True)),
                (
                    "created_by",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="prompt_labels_created",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "updated_by",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="prompt_labels_updated",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "workspace",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="prompt_labels",
                        to="db.workspace",
                    ),
                ),
            ],
            options={
                "db_table": "prompt_label",
                "ordering": ("name",),
            },
        ),
        migrations.CreateModel(
            name="PromptLabelAssignment",
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
                ("target_type", models.CharField(max_length=16)),
                ("target_key", models.CharField(max_length=64)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                (
                    "created_by",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="prompt_label_assignments_created",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "label",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="assignments",
                        to="prompting.promptlabel",
                    ),
                ),
            ],
            options={
                "db_table": "prompt_label_assignment",
            },
        ),
        migrations.AddIndex(
            model_name="promptlabel",
            index=models.Index(fields=["workspace"], name="prompt_label_ws_idx"),
        ),
        migrations.AddIndex(
            model_name="promptlabelassignment",
            index=models.Index(
                fields=["target_type", "target_key"],
                name="prompt_label_assign_target_idx",
            ),
        ),
        migrations.AddConstraint(
            model_name="promptlabel",
            constraint=models.UniqueConstraint(
                fields=["workspace", "name"],
                name="prompt_label_unique_name_per_ws",
            ),
        ),
        migrations.AddConstraint(
            model_name="promptlabelassignment",
            constraint=models.UniqueConstraint(
                fields=["label", "target_type", "target_key"],
                name="prompt_label_assignment_unique_target",
            ),
        ),
    ]
