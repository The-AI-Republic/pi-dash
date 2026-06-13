# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-section override storage + ``PromptTemplate`` workspace-row archival.

Introduces ``PromptSectionOverride`` (workspace- and user-scoped section
overrides). The composer now reads section defaults from code
(``prompting/sections/``) and overrides from this table; the legacy
``PromptTemplate`` table is no longer read at runtime. Its workspace-scoped
active rows are archived here (``is_active=False``, body preserved for
operator copy-out); the table drop is deferred one release. See design §6.1,
§8.2.
"""

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models
from django.db.models import Q


def archive_workspace_templates(apps, schema_editor):
    """Deactivate active workspace-scoped PromptTemplate rows.

    The global default (workspace IS NULL) is left untouched — it is no longer
    read but harmless, and dropping it is deferred with the table.
    """
    PromptTemplate = apps.get_model("prompting", "PromptTemplate")
    PromptTemplate.objects.filter(
        workspace__isnull=False, is_active=True
    ).update(is_active=False)


def noop_reverse(apps, schema_editor):
    # Reactivating archived rows on rollback is not meaningful — the composer no
    # longer reads them. Leave them archived.
    pass


class Migration(migrations.Migration):
    dependencies = [
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
        ("db", "0001_initial"),
        ("prompting", "0003_reseed_default_workpad_template"),
    ]

    operations = [
        migrations.CreateModel(
            name="PromptSectionOverride",
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
                ("section_key", models.CharField(max_length=64)),
                ("body", models.TextField()),
                ("is_active", models.BooleanField(default=True)),
                ("version", models.PositiveIntegerField(default=1)),
                ("needs_attention", models.BooleanField(default=False)),
                ("created_at", models.DateTimeField(auto_now_add=True)),
                ("updated_at", models.DateTimeField(auto_now=True)),
                (
                    "user",
                    models.ForeignKey(
                        blank=True,
                        help_text="NULL = workspace-level override; set = personal override.",
                        null=True,
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="prompt_section_overrides",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "updated_by",
                    models.ForeignKey(
                        blank=True,
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="prompt_section_overrides_updated",
                        to=settings.AUTH_USER_MODEL,
                    ),
                ),
                (
                    "workspace",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="prompt_section_overrides",
                        to="db.workspace",
                    ),
                ),
            ],
            options={
                "db_table": "prompt_section_override",
            },
        ),
        migrations.AddIndex(
            model_name="promptsectionoverride",
            index=models.Index(
                fields=["workspace", "user", "section_key", "is_active"],
                name="prompt_sec_overrid_ws_usr_idx",
            ),
        ),
        migrations.AddConstraint(
            model_name="promptsectionoverride",
            constraint=models.UniqueConstraint(
                condition=Q(is_active=True, user__isnull=True),
                fields=["workspace", "section_key"],
                name="prompt_section_override_one_active_ws",
            ),
        ),
        migrations.AddConstraint(
            model_name="promptsectionoverride",
            constraint=models.UniqueConstraint(
                condition=Q(is_active=True, user__isnull=False),
                fields=["workspace", "user", "section_key"],
                name="prompt_section_override_one_active_user",
            ),
        ),
        migrations.RunPython(archive_workspace_templates, noop_reverse),
    ]
