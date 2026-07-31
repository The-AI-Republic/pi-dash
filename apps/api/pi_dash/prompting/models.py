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


#: A label may be attached to a prompt *section* (keyed by ``section_key``) or a
#: *receipt* (keyed by prompt ``kind``). Sections/receipts are code-defined (see
#: ``prompting.registry`` / ``prompting.recipes``), not DB rows, so an assignment
#: references their stable string identifier rather than a foreign key.
TARGET_SECTION = "section"
TARGET_RECEIPT = "receipt"
TARGET_TYPES = frozenset({TARGET_SECTION, TARGET_RECEIPT})

#: Default chip color for a new label (matches the neutral grey used elsewhere).
DEFAULT_LABEL_COLOR = "#6b7280"


class PromptLabel(models.Model):
    """A workspace-scoped, user-managed label for prompt sections and receipts.

    Labels are shared across the workspace (like ``PromptSectionOverride``) and
    are deliberately kept separate from the issue ``Label`` model, which is
    project/issue-coupled and hierarchical. A label attaches to any number of
    sections/receipts via :class:`PromptLabelAssignment` (n:n).

    See PDASHOSS01-71.
    """

    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    workspace = models.ForeignKey(
        "db.Workspace",
        on_delete=models.CASCADE,
        related_name="prompt_labels",
    )
    name = models.CharField(max_length=64)
    color = models.CharField(max_length=32, default=DEFAULT_LABEL_COLOR)
    created_by = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        null=True,
        blank=True,
        on_delete=models.SET_NULL,
        related_name="prompt_labels_created",
    )
    updated_by = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        null=True,
        blank=True,
        on_delete=models.SET_NULL,
        related_name="prompt_labels_updated",
    )
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)

    class Meta:
        db_table = "prompt_label"
        ordering = ("name",)
        constraints = [
            models.UniqueConstraint(
                fields=["workspace", "name"],
                name="prompt_label_unique_name_per_ws",
            ),
        ]
        indexes = [models.Index(fields=["workspace"], name="prompt_label_ws_idx")]

    def __str__(self) -> str:
        return f"PromptLabel<ws={self.workspace_id}:{self.name}>"


class PromptLabelAssignment(models.Model):
    """A single attachment of a :class:`PromptLabel` to one section or receipt.

    ``target_type`` is ``section`` or ``receipt`` (see :data:`TARGET_TYPES`) and
    ``target_key`` is the section's ``section_key`` or the receipt's prompt
    ``kind``. The workspace scope is carried by the parent label.
    """

    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    label = models.ForeignKey(
        PromptLabel,
        on_delete=models.CASCADE,
        related_name="assignments",
    )
    target_type = models.CharField(max_length=16)
    target_key = models.CharField(max_length=64)
    created_by = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        null=True,
        blank=True,
        on_delete=models.SET_NULL,
        related_name="prompt_label_assignments_created",
    )
    created_at = models.DateTimeField(auto_now_add=True)

    class Meta:
        db_table = "prompt_label_assignment"
        constraints = [
            models.UniqueConstraint(
                fields=["label", "target_type", "target_key"],
                name="prompt_label_assignment_unique_target",
            ),
        ]
        indexes = [
            models.Index(
                fields=["target_type", "target_key"],
                name="prompt_label_assign_target_idx",
            )
        ]

    def __str__(self) -> str:
        return (
            f"PromptLabelAssignment<label={self.label_id}:"
            f"{self.target_type}:{self.target_key}>"
        )
