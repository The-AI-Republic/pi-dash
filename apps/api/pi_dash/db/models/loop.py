# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Loop (Auto Project Management) — instance-defined periodic assistant jobs.

A ``LoopJob`` is an instance-level (prompt + timer) authored by the operator.
A ``LoopTarget`` is the per-membership-edge cursor (job × workspace × user) that
the Beat scanner advances and fires; each fire dispatches a normal
``AssistantTurn`` in a hidden ``kind="loop"`` thread, running as the user with
the user's permissions and credentials. ``LoopUserPreference`` records a user's
opt-outs (absence of a row = enabled).

See ``.ai_design/loop_project_management/design.md`` §6.
"""

from __future__ import annotations

from django.conf import settings
from django.db import models

from .base import BaseModel


class SkipReason(models.TextChoices):
    """Why a due target was skipped instead of dispatched.

    Stored sparsely on :attr:`LoopTarget.last_skip_reason` (overwritten in
    place, never accreted as a log). See design §6.2.
    """

    USER_DISABLED = "user_disabled", "User disabled this job"
    MASTER_PAUSED = "master_paused", "User paused all Auto PM"
    MIN_ROLE = "min_role", "Below the job's minimum role"
    LLM_CONFIG_MISSING = "llm_config_missing", "No usable LLM credentials"
    MEMBERSHIP_GONE = "membership_gone", "No active workspace membership"
    TURN_ACTIVE = "turn_active", "Previous run still in flight"
    DISPATCH_ERROR = "dispatch_error", "Unexpected error creating the turn"


class LoopJob(BaseModel):
    """An instance catalog entry: a prompt + a recurrence.

    Instance-scoped (no workspace FK), so the operator defines a job once and it
    auto-applies to every membership edge via reconcile (design §7.2). The
    user-facing ``public_*`` fields never contain the word "loop"; ``name`` is
    the admin-facing label.
    """

    slug = models.CharField(max_length=64)
    name = models.CharField(max_length=255)
    public_name = models.CharField(max_length=255)
    public_description = models.TextField(blank=True, default="")
    prompt = models.TextField()
    # ROLE_CHOICES from db/models/workspace.py: 20 admin / 15 member / 5 guest.
    min_role = models.PositiveSmallIntegerField(default=15)
    enabled = models.BooleanField(default=True)
    is_builtin = models.BooleanField(default=True)

    # Timer — the same RRULE bundle subset as SchedulerBinding, evaluated by
    # pi_dash/bgtasks/_rrule.next_fire_from_rrule. ``rrule`` may not be empty:
    # a single-shot loop job is meaningless.
    dtstart = models.DateTimeField()
    rrule = models.CharField(max_length=255)
    tzid = models.CharField(max_length=64, default="UTC")

    class Meta:
        db_table = "loop_jobs"
        verbose_name = "Loop Job"
        verbose_name_plural = "Loop Jobs"
        ordering = ("-created_at",)
        constraints = [
            models.UniqueConstraint(
                fields=["slug"],
                condition=models.Q(deleted_at__isnull=True),
                name="loop_job_unique_slug_when_active",
            ),
        ]

    def __str__(self) -> str:
        return f"LoopJob({self.slug})"


class LoopTarget(BaseModel):
    """The cursor for one (job × workspace × user) membership edge.

    There is intentionally NO ``LoopRun`` model — the run *is* the
    ``AssistantTurn`` referenced by :attr:`last_run`; status, usage, errors, and
    the full transcript live there. See design §6.2.
    """

    job = models.ForeignKey("db.LoopJob", on_delete=models.CASCADE, related_name="targets")
    workspace = models.ForeignKey(
        "db.Workspace", on_delete=models.CASCADE, related_name="loop_targets"
    )
    user = models.ForeignKey(
        settings.AUTH_USER_MODEL, on_delete=models.CASCADE, related_name="loop_targets"
    )

    # The hidden conversation this target's runs land in. Recreated on rotation
    # (design §7.5); SET_NULL so deleting a thread can't kill the cursor.
    thread = models.ForeignKey(
        "assistant.AssistantThread",
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
        related_name="+",
    )

    # NULL = newly created, stagger pending (treated as due by the scanner).
    next_run_at = models.DateTimeField(null=True, blank=True)
    last_run = models.ForeignKey(
        "assistant.AssistantTurn",
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
        related_name="+",
    )
    # Sparse skip diagnostics — overwritten in place, never accreted.
    last_skipped_at = models.DateTimeField(null=True, blank=True)
    last_skip_reason = models.CharField(
        max_length=64, choices=SkipReason.choices, blank=True, default=""
    )

    class Meta:
        db_table = "loop_targets"
        verbose_name = "Loop Target"
        verbose_name_plural = "Loop Targets"
        ordering = ("-created_at",)
        constraints = [
            models.UniqueConstraint(
                fields=["job", "workspace", "user"],
                condition=models.Q(deleted_at__isnull=True),
                name="loop_target_unique_edge_when_active",
            ),
        ]
        indexes = [models.Index(fields=["next_run_at"], name="loop_target_due_idx")]

    def __str__(self) -> str:
        return f"LoopTarget(job={self.job_id}, ws={self.workspace_id}, user={self.user_id})"


class LoopUserPreference(BaseModel):
    """A user's opt-out for a job (or the master "pause all" switch when
    ``job`` is NULL). Absence of a row means enabled — only opt-outs are stored,
    which is what makes new builtin jobs light up with zero backfill (design
    §6.3).
    """

    user = models.ForeignKey(
        settings.AUTH_USER_MODEL, on_delete=models.CASCADE, related_name="loop_preferences"
    )
    # NULL job = the master "pause all Auto Project Management" switch.
    job = models.ForeignKey(
        "db.LoopJob",
        on_delete=models.CASCADE,
        null=True,
        blank=True,
        related_name="user_preferences",
    )
    enabled = models.BooleanField(default=True)

    class Meta:
        db_table = "loop_user_preferences"
        verbose_name = "Loop User Preference"
        verbose_name_plural = "Loop User Preferences"
        ordering = ("-created_at",)
        constraints = [
            models.UniqueConstraint(
                fields=["user", "job"],
                condition=models.Q(deleted_at__isnull=True),
                name="loop_pref_unique_user_job_when_active",
            ),
            models.UniqueConstraint(
                fields=["user"],
                condition=models.Q(job__isnull=True, deleted_at__isnull=True),
                name="loop_pref_unique_user_master_when_active",
            ),
        ]

    def __str__(self) -> str:
        scope = self.job_id or "master"
        return f"LoopUserPreference(user={self.user_id}, job={scope}, enabled={self.enabled})"
