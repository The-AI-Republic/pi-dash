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
