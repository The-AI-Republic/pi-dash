# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Prompt template storage.

See `.ai_design/prompt_system/prompt-system-design.md` §4 for the model shape
and lookup semantics: one active row per (workspace, name) with a NULL-workspace
row acting as the global default.
"""

import uuid

from django.conf import settings
from django.db import models
from django.db.models import Q


class PromptTemplate(models.Model):
    DEFAULT_NAME = "coding-task"

    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    workspace = models.ForeignKey(
        "db.Workspace",
        null=True,
        blank=True,
        on_delete=models.CASCADE,
        related_name="prompt_templates",
        help_text="NULL = global default template.",
    )
    name = models.CharField(max_length=64, default=DEFAULT_NAME)
    body = models.TextField()
    is_active = models.BooleanField(default=True)
    version = models.PositiveIntegerField(default=1)
    updated_by = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        null=True,
        blank=True,
        on_delete=models.SET_NULL,
        related_name="prompt_templates_updated",
    )
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)

    class Meta:
        db_table = "prompt_template"
        constraints = [
            models.UniqueConstraint(
                fields=["workspace", "name"],
                condition=Q(is_active=True),
                name="prompt_template_one_active_per_ws_name",
            ),
        ]
        indexes = [models.Index(fields=["workspace", "name", "is_active"])]

    def __str__(self) -> str:
        scope = f"ws={self.workspace_id}" if self.workspace_id else "global"
        return f"PromptTemplate<{scope}:{self.name}:v{self.version}>"

    @property
    def is_global_default(self) -> bool:
        return self.workspace_id is None


class PromptSectionOverride(models.Model):
    """A workspace- or user-scoped override of one prompt section's body.

    The registry (``prompting.registry``) owns default section bodies in code;
    this table stores *only* the deltas. Resolution precedence (see
    ``prompting.composer.resolve_section``) is user override → workspace
    override → registry default. ``user IS NULL`` is the workspace-level row.

    See ``.ai_design/prompt_section_system/design.md`` §6.1.
    """

    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    workspace = models.ForeignKey(
        "db.Workspace",
        on_delete=models.CASCADE,
        related_name="prompt_section_overrides",
    )
    user = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        null=True,
        blank=True,
        on_delete=models.CASCADE,
        related_name="prompt_section_overrides",
        help_text="NULL = workspace-level override; set = personal override.",
    )
    section_key = models.CharField(max_length=64)
    body = models.TextField()
    is_active = models.BooleanField(default=True)
    version = models.PositiveIntegerField(default=1)
    # Set by the re-validation command when a registry/context change would
    # break this override at render time (design §6.4). Never auto-deactivated.
    needs_attention = models.BooleanField(default=False)
    updated_by = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        null=True,
        blank=True,
        on_delete=models.SET_NULL,
        related_name="prompt_section_overrides_updated",
    )
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)

    class Meta:
        db_table = "prompt_section_override"
        constraints = [
            # Postgres treats NULLs as DISTINCT in unique indexes, and
            # ``nulls_distinct=False`` needs Django 5.0+ / PG 15+ (repo is on
            # Django 4.2). A single constraint over (workspace, user,
            # section_key) would therefore allow unlimited duplicate ACTIVE
            # workspace-level rows (user IS NULL). Split into two partial
            # constraints — one per scope — instead. See design §6.1.
            models.UniqueConstraint(
                fields=["workspace", "section_key"],
                condition=Q(is_active=True, user__isnull=True),
                name="prompt_section_override_one_active_ws",
            ),
            models.UniqueConstraint(
                fields=["workspace", "user", "section_key"],
                condition=Q(is_active=True, user__isnull=False),
                name="prompt_section_override_one_active_user",
            ),
        ]
        indexes = [
            models.Index(
                fields=["workspace", "user", "section_key", "is_active"],
                name="prompt_sec_overrid_ws_usr_idx",
            )
        ]

    def __str__(self) -> str:
        scope = f"user={self.user_id}" if self.user_id else "workspace"
        return (
            f"PromptSectionOverride<ws={self.workspace_id}:{scope}:"
            f"{self.section_key}:v{self.version}>"
        )

    @property
    def is_workspace_level(self) -> bool:
        return self.user_id is None
