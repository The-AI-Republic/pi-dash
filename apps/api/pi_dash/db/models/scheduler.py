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


# Cap on ``SchedulerBinding.last_error`` (and any operator-facing copy of
# the same field). Truncation lives in one place so the scanner, the
# dispatch path, and the runner-side terminate hook stay in sync.
LAST_ERROR_MAX_LEN = 1000


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
    # Display color, surfaced by the project calendar (PR3). Stored as a
    # 7-char hex string ("#rrggbb"). Chosen by workspace admin from the
    # fixed 16-color palette; freeform hex is accepted.
    color = models.CharField(max_length=7, default="#3b82f6")

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

    # iCal-shaped recurrence (RFC 5545). Replaces the legacy cron string
    # in migration 0140. See .ai_design/project_scheduler_calendar/decisions.md §1.
    # - ``dtstart`` is the series anchor (tz-aware UTC).
    # - ``tzid`` is stored for future wall-clock-aware expansion but is
    #   currently informational; expansion runs in UTC.
    # - ``rrule`` empty = single-shot at dtstart.
    # - ``rdates`` / ``exdates`` are JSON arrays of ISO datetime strings.
    dtstart = models.DateTimeField()
    tzid = models.CharField(max_length=64, default="UTC")
    rrule = models.TextField(blank=True, default="")
    rdates = models.JSONField(default=list, blank=True)
    exdates = models.JSONField(default=list, blank=True)

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

    # Optional pod override for runs fired by this binding. NULL means "use
    # the project's default pod" (resolved at fire time). The pod is *late
    # bound* — the dispatcher reads this on each fire rather than pinning a
    # run, so changing it takes effect on the next tick. SET_NULL so a hard
    # pod delete degrades to the project default rather than orphaning the
    # binding; the dispatcher additionally falls back when the pod is
    # soft-deleted or no longer in this project.
    pod = models.ForeignKey(
        "runner.Pod",
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
        related_name="scheduler_bindings",
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
