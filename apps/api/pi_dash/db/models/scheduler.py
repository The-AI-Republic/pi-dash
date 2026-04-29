# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Project Scheduler — definitions and per-project installs.

See ``.ai_design/project_scheduler/design.md`` §5.
"""

from __future__ import annotations

from django.conf import settings
from django.db import models

from .base import BaseModel
from .workspace import WorkspaceBaseModel


class SchedulerSource(models.TextChoices):
    BUILTIN = "builtin", "Builtin"
    MANIFEST = "manifest", "Manifest"


class Scheduler(BaseModel):
    """A reusable scheduler definition: a slug + prompt + workspace-level
    enable flag. Definitions are workspace-scoped (mirrors
    ``WorkspaceIntegration``); projects install them via
    :class:`SchedulerBinding`.
    """

    workspace = models.ForeignKey(
        "db.Workspace",
        on_delete=models.CASCADE,
        related_name="schedulers",
    )
    slug = models.CharField(max_length=64)
    name = models.CharField(max_length=255)
    description = models.TextField(blank=True, default="")
    prompt = models.TextField()
    source = models.CharField(
        max_length=16,
        choices=SchedulerSource.choices,
        default=SchedulerSource.BUILTIN,
    )
    is_enabled = models.BooleanField(default=True)

    class Meta:
        db_table = "schedulers"
        verbose_name = "Scheduler"
        verbose_name_plural = "Schedulers"
        ordering = ("-created_at",)
        # BaseModel inherits SoftDeleteModel (deleted_at), so a plain
        # unique_together would collide with tombstones on uninstall/reinstall.
        # Match GithubRepositorySync (db/models/integration/github.py).
        constraints = [
            models.UniqueConstraint(
                fields=["workspace", "slug"],
                condition=models.Q(deleted_at__isnull=True),
                name="scheduler_unique_workspace_slug_when_active",
            ),
        ]

    def __str__(self) -> str:
        return f"Scheduler({self.workspace_id}/{self.slug})"


class SchedulerBinding(WorkspaceBaseModel):
    """An install of one Scheduler onto one Project.

    Carries the per-install cadence (``cron``), optional extra prompt
    context appended at run time, and the runtime state used by the Beat
    fire loop (``next_run_at``, ``last_run``).
    """

    scheduler = models.ForeignKey(
        Scheduler,
        on_delete=models.CASCADE,
        related_name="bindings",
    )

    cron = models.CharField(max_length=64)
    extra_context = models.TextField(blank=True, default="")
    enabled = models.BooleanField(default=True)

    next_run_at = models.DateTimeField(null=True, blank=True)
    # Single source of truth for "last run state": the AgentRun itself.
    # The binding does NOT carry a duplicated status enum — read the status
    # off ``last_run.status``.
    last_run = models.ForeignKey(
        "runner.AgentRun",
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
        related_name="+",
    )
    # Short-circuit errors that never produced a run (no default pod, etc.).
    last_error = models.TextField(blank=True, default="")

    actor = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        on_delete=models.SET_NULL,
        null=True,
        related_name="scheduler_bindings_authored",
    )

    class Meta:
        db_table = "scheduler_bindings"
        verbose_name = "Scheduler Binding"
        verbose_name_plural = "Scheduler Bindings"
        ordering = ("-created_at",)
        constraints = [
            models.UniqueConstraint(
                fields=["scheduler", "project"],
                condition=models.Q(deleted_at__isnull=True),
                name="scheduler_binding_unique_per_project_when_active",
            ),
        ]
        indexes = [
            models.Index(
                fields=["enabled", "next_run_at"],
                name="sched_binding_due_idx",
            ),
        ]

    def __str__(self) -> str:
        return f"SchedulerBinding(scheduler={self.scheduler_id}, project={self.project_id})"
