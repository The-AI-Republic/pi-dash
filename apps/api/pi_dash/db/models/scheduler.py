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


class OutcomeMode(models.TextChoices):
    """What a scheduler run should *do* with whatever it finds.

    Stored per-install on :attr:`SchedulerBinding.outcome_mode` (not on the
    workspace ``Scheduler``), so the same scheduler can behave differently
    across projects. The scheduler layer never creates issues or edits code
    itself — it only dispatches an agent run. ``outcome_mode`` steers that run
    by appending a work-mode directive to the dispatched prompt (see
    ``OUTCOME_MODE_DIRECTIVES`` and ``pi_dash.prompting.context.
    build_scheduler_task_body``). The agent still does the work via the same
    ``pidash`` CLI / git tools a user-driven run uses.
    """

    CREATE_ISSUE = "create_issue", "Create issues"
    APPLY_FIX = "apply_fix", "Apply fix (open PR)"
    FIX_AND_REVIEW = "fix_and_review", "Fix & open for review"


#: Work-mode directive appended to a scheduler run's prompt, keyed by
#: ``OutcomeMode``. Single source of truth so the dispatcher, tests, and any
#: future composer share the same wording. Kept terse — the scheduler's own
#: prompt carries the task; this only fixes *what to do with findings*.
OUTCOME_MODE_DIRECTIVES: dict[str, str] = {
    OutcomeMode.CREATE_ISSUE: (
        "## Work mode: create issues\n\n"
        "For each distinct finding, file a Pi Dash issue with the `pidash` CLI:\n"
        "    pidash issue create --project <PROJ> --title \"<short summary>\" \\\n"
        "        --description \"<file path, line range, evidence, severity, "
        'suggested fix>"\n'
        "Before creating an issue, list existing open issues and skip any "
        "finding that already has a corresponding open issue (de-dupe by file "
        "+ root cause, not by exact title). Do NOT modify code."
    ),
    OutcomeMode.APPLY_FIX: (
        "## Work mode: apply fix\n\n"
        "For each finding you are confident about, implement the fix and open a "
        "pull request for human review — do NOT merge it. Keep one PR per "
        "logical fix where practical. If a fix is risky, ambiguous, or larger "
        "than a focused change, do NOT force it: create a Pi Dash issue "
        "describing the finding instead (same form as create-issue mode)."
    ),
    OutcomeMode.FIX_AND_REVIEW: (
        "## Work mode: file issue and delegate fix\n\n"
        "Do NOT modify code or open a pull request in this run — the fix is "
        "delegated to the issue agent. For each distinct finding, do ALL of "
        "the following:\n"
        "1. File a Pi Dash issue with the `pidash` CLI (de-dupe against existing "
        "open issues by file + root cause, as in create-issue mode), and note "
        "the issue identifier it returns. Write the description so an AI agent "
        "can implement the fix without re-investigating: file path(s) and line "
        "range, the evidence you observed, root cause, severity, a concrete "
        "suggested fix, and how to validate it:\n"
        "    pidash issue create --project <PROJ> --title \"<short summary>\" \\\n"
        "        --description \"<agent-ready technical details>\"\n"
        "2. Move the issue to In Progress — this automatically delegates it to "
        "the coding agent, which implements the fix and opens a pull request "
        "for human review:\n"
        "    pidash issue patch <IDENT> --state \"In Progress\"\n"
        "If a finding is risky, ambiguous, or larger than a focused change, "
        "still file the issue but leave it in its default state (do NOT move "
        "it to In Progress) and describe the open questions in the issue "
        "description instead."
    ),
}


def outcome_mode_directive(mode: str) -> str:
    """Return the prompt directive for ``mode``.

    Falls back to the ``CREATE_ISSUE`` directive for an unknown value so a
    stale row can never dispatch a run with no work-mode guidance at all.
    """
    return OUTCOME_MODE_DIRECTIVES.get(
        mode, OUTCOME_MODE_DIRECTIVES[OutcomeMode.CREATE_ISSUE]
    )


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

    # What a run of THIS install does with its findings (file issues / open
    # fix PRs / fix + move to review). Lives on the binding, not the Scheduler,
    # so the same workspace scheduler can behave differently per project. Steers
    # the dispatched prompt (see OutcomeMode); defaults to CREATE_ISSUE to match
    # the pre-existing builtin behavior.
    outcome_mode = models.CharField(
        max_length=16,
        choices=OutcomeMode.choices,
        default=OutcomeMode.CREATE_ISSUE,
    )

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
