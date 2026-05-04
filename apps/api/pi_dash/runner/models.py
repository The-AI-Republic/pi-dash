# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import uuid

from django.conf import settings
from django.db import models
from django.utils import timezone


class PodManager(models.Manager):
    """Default manager: excludes soft-deleted pods from routine queries.

    Use ``Pod.all_objects`` to include tombstones in admin / audit views.
    """

    def get_queryset(self):
        return super().get_queryset().filter(deleted_at__isnull=True)


class Pod(models.Model):
    """A project-scoped group of runners that share a work queue.

    See ``.ai_design/n_runners_in_same_machine/new_pod_project_relationship/design.md``
    §5–§6 for the project-scoped model, and ``.ai_design/issue_runner/design.md``
    §4.1 for the historical workspace-scoped model that this replaces.

    A pod belongs to exactly one project; one project can own many
    pods. Each project has exactly one ``is_default=True`` pod,
    auto-created on Project save (see :mod:`pi_dash.runner.signals`).
    Non-default pods exist for tier / region / branch separation and
    are first-class citizens in the data model — but routing rules
    that send specific issues to non-default pods are deferred (see
    decisions.md Q7).
    """

    MAX_PER_PROJECT = 20

    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    workspace = models.ForeignKey(
        "db.Workspace",
        on_delete=models.CASCADE,
        related_name="pods",
    )
    project = models.ForeignKey(
        "db.Project",
        on_delete=models.CASCADE,
        related_name="pods",
    )
    name = models.CharField(max_length=128)
    description = models.CharField(max_length=512, blank=True, default="")
    created_by = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
        related_name="pods_created",
    )
    is_default = models.BooleanField(default=False)
    deleted_at = models.DateTimeField(null=True, blank=True, db_index=True)
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)

    objects = PodManager()
    all_objects = models.Manager()

    class Meta:
        db_table = "pod"
        ordering = ("-is_default", "created_at")
        constraints = [
            # Pod names are unique-per-project, not per-workspace. Two
            # projects in the same workspace can each have a "WEB_pod_1"
            # and an "API_pod_1" without colliding.
            models.UniqueConstraint(
                fields=["project", "name"],
                condition=models.Q(deleted_at__isnull=True),
                name="pod_unique_name_per_project_when_active",
            ),
            # Exactly one default pod per project at any time. The
            # post_save(Project) signal creates the first one; transferring
            # the default flag to another pod is a future operation.
            models.UniqueConstraint(
                fields=["project"],
                condition=models.Q(is_default=True) & models.Q(deleted_at__isnull=True),
                name="pod_one_default_per_project_when_active",
            ),
        ]
        indexes = [
            models.Index(
                fields=["project", "is_default"], name="pod_project_is_def_idx"
            ),
            models.Index(
                fields=["workspace", "is_default"], name="pod_workspc_is_def_idx"
            ),
        ]

    def __str__(self) -> str:
        return f"{self.name} (project={self.project_id})"

    def clean(self):
        """Enforce ``pod.workspace_id == pod.project.workspace_id``.

        ``workspace`` is a denormalised convenience used by dashboard
        queries that don't want to traverse the project FK; the ground
        truth is ``project.workspace``. Catching the mismatch at clean
        time keeps the denorm honest.
        """
        super().clean()
        if self.project_id is not None:
            if self.workspace_id is None:
                # Auto-fill from the project on save when omitted.
                self.workspace_id = self.project.workspace_id
            elif self.workspace_id != self.project.workspace_id:
                from django.core.exceptions import ValidationError

                raise ValidationError(
                    {
                        "workspace": (
                            "pod.workspace must match pod.project.workspace"
                        )
                    }
                )

    def save(self, *args, **kwargs):
        # Auto-fill workspace from project so callers don't have to set
        # both, and enforce the denorm invariant directly here. Django's
        # save() doesn't invoke clean(), and no Pod call site runs
        # full_clean(), so the equality check has to live on the
        # persistence path or it's purely advisory.
        if self.project_id is not None:
            project_workspace_id = self.project.workspace_id
            if self.workspace_id is None:
                self.workspace_id = project_workspace_id
            elif self.workspace_id != project_workspace_id:
                from django.core.exceptions import ValidationError

                raise ValidationError(
                    {
                        "workspace": (
                            "pod.workspace must match pod.project.workspace"
                        )
                    }
                )
        super().save(*args, **kwargs)

    @classmethod
    def default_for_project(cls, project) -> "Pod | None":
        """Return the active default pod for a project, or None.

        Every project normally has exactly one default pod (auto-created
        by :func:`pi_dash.runner.signals.create_default_pod_for_new_project`).
        Returns None only during the transient window between Project
        save and the post_save signal firing, or when an admin has
        soft-deleted the default without promoting a replacement.
        """
        return cls.default_for_project_id(project.id)

    @classmethod
    def default_for_project_id(cls, project_id) -> "Pod | None":
        """Same as :meth:`default_for_project` but avoids loading the Project."""
        return cls.objects.filter(project_id=project_id, is_default=True).first()


class RunnerStatus(models.TextChoices):
    ONLINE = "online", "Online"
    OFFLINE = "offline", "Offline"
    BUSY = "busy", "Busy"
    REVOKED = "revoked", "Revoked"


class AgentRunStatus(models.TextChoices):
    QUEUED = "queued", "Queued"
    ASSIGNED = "assigned", "Assigned"
    RUNNING = "running", "Running"
    AWAITING_APPROVAL = "awaiting_approval", "Awaiting Approval"
    AWAITING_REAUTH = "awaiting_reauth", "Awaiting Reauth"
    PAUSED_AWAITING_INPUT = "paused_awaiting_input", "Paused — Awaiting Input"
    BLOCKED = "blocked", "Blocked"
    COMPLETED = "completed", "Completed"
    FAILED = "failed", "Failed"
    CANCELLED = "cancelled", "Cancelled"


class ApprovalStatus(models.TextChoices):
    PENDING = "pending", "Pending"
    ACCEPTED = "accepted", "Accepted"
    DECLINED = "declined", "Declined"
    EXPIRED = "expired", "Expired"


class ApprovalKind(models.TextChoices):
    COMMAND_EXECUTION = "command_execution", "Command Execution"
    FILE_CHANGE = "file_change", "File Change"
    NETWORK_ACCESS = "network_access", "Network Access"
    OTHER = "other", "Other"


class Runner(models.Model):
    """First-class trust and worker entity (per-runner HTTPS transport).

    Each runner owns its own refresh token, access-token generation, and
    revocation state. The legacy ``Connection`` row that used to wrap a
    machine + N runners is gone; a runner is the unit of trust, auth, and
    delivery ownership. See ``.ai_design/move_to_https/design.md`` §4-§6.
    """

    MAX_PER_USER = 5

    id = models.UUIDField(
        primary_key=True, default=uuid.uuid4, editable=False, db_index=True
    )
    owner = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        on_delete=models.CASCADE,
        related_name="runners",
    )
    workspace = models.ForeignKey(
        "db.Workspace",
        on_delete=models.CASCADE,
        related_name="runners",
    )
    # Every runner belongs to exactly one pod (§4.2). PROTECT because pods are
    # soft-deleted, not physically removed, so this FK is always valid.
    pod = models.ForeignKey(
        Pod,
        on_delete=models.PROTECT,
        related_name="runners",
    )
    name = models.CharField(max_length=128)
    # Free-form host hint reported at enrollment time; surfaced in the UI.
    host_label = models.CharField(max_length=255, blank=True, default="")
    # Refresh-token hash and rotation state per ``design.md`` §5.3 / §6.1.
    refresh_token_hash = models.CharField(max_length=128, blank=True, default="", db_index=True)
    refresh_token_fingerprint = models.CharField(max_length=16, blank=True, default="")
    refresh_token_generation = models.PositiveIntegerField(default=0)
    # Single-slot replay-detection window; matches the previous-hash
    # decision in ``design.md`` §5.3 step 3.
    previous_refresh_token_hash = models.CharField(max_length=128, blank=True, default="")
    # Reserved for future Ed25519 / sidecar-verifier rollout.
    access_token_signing_key_version = models.PositiveIntegerField(default=1)
    # One-time enrollment token state; cleared once the daemon exchanges
    # it for the long-lived refresh token.
    enrollment_token_hash = models.CharField(max_length=128, blank=True, default="")
    enrollment_token_fingerprint = models.CharField(max_length=16, blank=True, default="")
    enrolled_at = models.DateTimeField(null=True, blank=True)
    capabilities = models.JSONField(default=list, blank=True)
    status = models.CharField(
        max_length=16,
        choices=RunnerStatus.choices,
        default=RunnerStatus.OFFLINE,
        db_index=True,
    )
    os = models.CharField(max_length=32, blank=True, default="")
    arch = models.CharField(max_length=32, blank=True, default="")
    runner_version = models.CharField(max_length=32, blank=True, default="")
    protocol_version = models.PositiveIntegerField(default=1)
    last_heartbeat_at = models.DateTimeField(null=True, blank=True)
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)
    revoked_at = models.DateTimeField(null=True, blank=True)
    revoked_reason = models.CharField(max_length=32, blank=True, default="")

    class Meta:
        db_table = "runner"
        ordering = ("-last_heartbeat_at", "-created_at")
        # Per-pod name uniqueness (§4.2). Pod is the natural namespace; the CLI
        # still addresses runners by name within a pod. Two pods in the same
        # workspace can each have a runner named "mac-mini".
        constraints = [
            models.UniqueConstraint(
                fields=["pod", "name"],
                name="runner_unique_name_per_pod",
            ),
        ]
        indexes = [
            models.Index(fields=["owner", "status"]),
            models.Index(fields=["workspace", "status"]),
            models.Index(fields=["pod", "status"], name="runner_pod_status_idx"),
        ]

    def __str__(self) -> str:
        return f"{self.name} ({self.owner_id})"

    def save(self, *args, **kwargs):
        # Production runner registration (`register/`, `register-under-token/`)
        # resolves a specific project's pod before instantiating the Runner.
        # As a convenience for direct-ORM callers (tests, management
        # commands, single-project workspaces), if `pod` is omitted and
        # the workspace has exactly one project with a default pod, we
        # auto-resolve to it. Workspaces with multiple projects refuse
        # to auto-resolve — those callers must pick a project explicitly.
        if self.pod_id is None and self.workspace_id is not None:
            from pi_dash.db.models.project import Project

            project_ids = list(
                Project.objects.filter(workspace_id=self.workspace_id)
                .values_list("id", flat=True)[:2]
            )
            if len(project_ids) == 1:
                default = Pod.default_for_project_id(project_ids[0])
                if default is not None:
                    self.pod = default
        super().save(*args, **kwargs)

    @property
    def project(self):
        """Convenience accessor — derived from ``runner.pod.project``.

        Not a denorm column. The source of truth is the pod's project FK;
        adding a direct ``Runner.project`` FK would invite drift (e.g. if
        a runner's pod is reassigned). Hot-path queries that need the
        project filter should join via ``runner.pod`` (one indexed join).
        """
        return self.pod.project if self.pod_id is not None else None

    @property
    def project_id(self):
        if self.pod_id is None:
            return None
        return self.pod.project_id

    def mark_heartbeat(self) -> None:
        self.last_heartbeat_at = timezone.now()
        self.save(update_fields=["last_heartbeat_at"])

    def revoke(self, reason: str = "manual_revoke") -> None:
        """Mark the runner revoked and cascade to sessions / runs / pins.

        Cascade order (``design.md`` §6.3, §7.8):

        1. Revoke any active ``RunnerSession`` for this runner (so the
           daemon's next poll receives ``409 session_evicted``).
        2. Cancel non-terminal ``AgentRun`` rows owned by this runner.
        3. Unpin QUEUED follow-ups so they flow back into the pod queue.
        4. Schedule delayed Redis stream cleanup once the daemon has had
           a brief chance to observe shutdown (``design.md`` §7.8).

        The reason string is one of ``manual_revoke``,
        ``membership_revoked``, ``refresh_token_replayed``, or
        ``runner_removed``.
        """
        from django.db import transaction
        from pi_dash.runner.services.matcher import (
            NON_TERMINAL_STATUSES,
            drain_pod_by_id,
        )

        if self.revoked_at is not None:
            return

        affected_pod_ids: set = set()
        with transaction.atomic():
            now = timezone.now()
            Runner.objects.filter(pk=self.pk).update(
                status=RunnerStatus.REVOKED,
                revoked_at=now,
                revoked_reason=reason[:32],
            )
            self.status = RunnerStatus.REVOKED
            self.revoked_at = now
            self.revoked_reason = reason[:32]

            # (1) Revoke any active RunnerSession for this runner. The
            # next daemon poll on the old session will see the row gone
            # and react with 409 session_evicted; pub/sub eviction
            # signaling fires from the eviction sweeper / session-open
            # path, not here.
            RunnerSession.objects.filter(
                runner=self, revoked_at__isnull=True
            ).update(revoked_at=now, revoked_reason="runner_revoked")

            active_runs = list(
                AgentRun.objects.select_for_update()
                .filter(runner=self, status__in=NON_TERMINAL_STATUSES)
                .values_list("pk", "pod_id")
            )
            if active_runs:
                AgentRun.objects.filter(
                    pk__in=[pk for pk, _ in active_runs]
                ).update(
                    status=AgentRunStatus.CANCELLED,
                    ended_at=now,
                    error="runner revoked",
                )
                affected_pod_ids = {pid for _, pid in active_runs if pid is not None}

            pinned_pod_ids = list(
                AgentRun.objects.filter(
                    pinned_runner=self, status=AgentRunStatus.QUEUED
                ).values_list("pod_id", flat=True)
            )
            if pinned_pod_ids:
                AgentRun.objects.filter(
                    pinned_runner=self, status=AgentRunStatus.QUEUED
                ).update(pinned_runner=None)
                affected_pod_ids.update(pid for pid in pinned_pod_ids if pid is not None)

        for pod_id in affected_pod_ids:
            transaction.on_commit(lambda pid=pod_id: drain_pod_by_id(pid))

        # Schedule delayed stream cleanup. We do not destroy the stream
        # inline because a still-live daemon may want to drain a final
        # `revoke` control frame before its session disappears.
        from pi_dash.runner.services.outbox import (
            schedule_stream_cleanup_for_runner,
        )

        transaction.on_commit(
            lambda rid=self.pk: schedule_stream_cleanup_for_runner(rid)
        )

class RunnerSession(models.Model):
    """Per-runner cloud session: owns delivery for one runner.

    See ``.ai_design/move_to_https/design.md`` §6.3.

    A session row is created on ``POST /runners/<rid>/sessions/`` and
    revoked on session-eviction, idle-timeout, or runner revocation.
    Exactly one active session per runner is enforced via a partial
    unique constraint.
    """

    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    runner = models.ForeignKey(
        "Runner", on_delete=models.CASCADE, related_name="sessions"
    )
    protocol_version = models.PositiveIntegerField(default=4)
    created_at = models.DateTimeField(auto_now_add=True)
    last_seen_at = models.DateTimeField(null=True, blank=True)
    revoked_at = models.DateTimeField(null=True, blank=True)
    revoked_reason = models.CharField(max_length=32, blank=True, default="")

    class Meta:
        db_table = "runner_session"
        ordering = ("-created_at",)
        constraints = [
            models.UniqueConstraint(
                fields=["runner"],
                condition=models.Q(revoked_at__isnull=True),
                name="runner_session_one_active_per_runner",
            ),
        ]
        indexes = [
            models.Index(fields=["runner", "revoked_at"]),
            models.Index(fields=["last_seen_at"]),
        ]


class RunnerForceRefresh(models.Model):
    """Force-refresh directive for a runner.

    See ``.ai_design/move_to_https/design.md`` §5.2 / §7.8. While a row
    exists, the access-token verifier rejects tokens whose
    ``rtg < min_rtg``. The row is deleted on the runner's next
    successful refresh.
    """

    runner = models.OneToOneField(
        "Runner",
        on_delete=models.CASCADE,
        related_name="force_refresh",
        primary_key=True,
    )
    min_rtg = models.PositiveIntegerField(default=0)
    reason = models.CharField(max_length=64, blank=True, default="")
    created_at = models.DateTimeField(auto_now_add=True)

    class Meta:
        db_table = "runner_force_refresh"


class RunMessageDedupe(models.Model):
    """Idempotency record for runner→cloud HTTP run-lifecycle POSTs.

    See ``.ai_design/move_to_https/design.md`` §7.5. Keyed on
    ``(run, message_id)``: a duplicate POST short-circuits to the
    cached response status without re-applying the side effects.
    """

    run = models.ForeignKey(
        "AgentRun", on_delete=models.CASCADE, related_name="message_dedupes"
    )
    message_id = models.CharField(max_length=128)
    created_at = models.DateTimeField(auto_now_add=True)

    class Meta:
        db_table = "run_message_dedupe"
        constraints = [
            models.UniqueConstraint(
                fields=["run", "message_id"],
                name="run_message_dedupe_unique",
            ),
        ]
        indexes = [models.Index(fields=["created_at"])]


class MachineToken(models.Model):
    """Machine-scoped CLI credential ("pidash auth login").

    See ``.ai_design/move_to_https/design.md`` §5.6. Independent of
    runner transport: a machine has at most one active MachineToken
    per ``(user, workspace, host_label)`` regardless of how many
    runners it hosts.
    """

    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    user = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        on_delete=models.CASCADE,
        related_name="machine_tokens",
    )
    workspace = models.ForeignKey(
        "db.Workspace",
        on_delete=models.CASCADE,
        related_name="machine_tokens",
    )
    host_label = models.CharField(max_length=255)
    token_hash = models.CharField(max_length=128, db_index=True)
    token_fingerprint = models.CharField(max_length=16, blank=True, default="")
    label = models.CharField(max_length=128, blank=True, default="")
    is_service = models.BooleanField(default=True)
    created_at = models.DateTimeField(auto_now_add=True)
    last_used_at = models.DateTimeField(null=True, blank=True)
    revoked_at = models.DateTimeField(null=True, blank=True)

    class Meta:
        db_table = "machine_token"
        ordering = ("-created_at",)
        constraints = [
            models.UniqueConstraint(
                fields=["user", "workspace", "host_label"],
                condition=models.Q(revoked_at__isnull=True),
                name="machine_token_one_active_per_user_ws_host",
            ),
        ]
        indexes = [
            models.Index(fields=["user", "workspace", "revoked_at"]),
        ]

    def revoke(self) -> None:
        if self.revoked_at is not None:
            return
        self.revoked_at = timezone.now()
        self.save(update_fields=["revoked_at"])


class AgentRun(models.Model):
    id = models.UUIDField(
        primary_key=True, default=uuid.uuid4, editable=False, db_index=True
    )
    workspace = models.ForeignKey(
        "db.Workspace",
        on_delete=models.CASCADE,
        related_name="agent_runs",
    )
    # `owner` = billable party; populated at assignment from runner.owner (§5.3).
    # Nullable because it's unknown until a runner is assigned. Legacy rows
    # that had owner pre-set under the old model are unaffected.
    owner = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
        related_name="agent_runs",
    )
    # `created_by` = the user who triggered the run. Authoritative principal for
    # list / detail / cancel / approval permissions (decision #9).
    created_by = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        on_delete=models.PROTECT,
        related_name="agent_runs_created",
        null=False,
    )
    # Every run belongs to exactly one pod; resolved before insert (§6.5).
    pod = models.ForeignKey(
        "runner.Pod",
        on_delete=models.PROTECT,
        related_name="agent_runs",
    )
    runner = models.ForeignKey(
        Runner,
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
        related_name="agent_runs",
    )
    # Soft affinity: when set, dispatch routes this QUEUED run only to this
    # runner. Used by comment-triggered continuations so a follow-up resumes
    # on the same runner that holds the prior session on disk. Cleared when
    # the runner is revoked or by an operator escape hatch (see §5.7 of
    # .ai_design/issue_run_improve/design.md).
    pinned_runner = models.ForeignKey(
        Runner,
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
        related_name="pinned_agent_runs",
    )
    work_item = models.ForeignKey(
        "db.Issue",
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
        related_name="agent_runs",
    )
    # Project-scoped runs (scheduler ticks) carry this back-pointer instead of
    # ``work_item``. Exactly one of ``work_item`` / ``scheduler_binding`` is
    # set per run; the dispatcher enforces the invariant.
    scheduler_binding = models.ForeignKey(
        "db.SchedulerBinding",
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
        related_name="agent_runs",
    )
    parent_run = models.ForeignKey(
        "self",
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
        related_name="follow_up_runs",
        help_text="Prior run this attempt follows up on; null for an issue's initial run.",
    )
    status = models.CharField(
        max_length=24,
        choices=AgentRunStatus.choices,
        default=AgentRunStatus.QUEUED,
        db_index=True,
    )
    prompt = models.TextField(blank=True, default="")
    run_config = models.JSONField(default=dict, blank=True)
    required_capabilities = models.JSONField(default=list, blank=True)
    thread_id = models.CharField(max_length=128, blank=True, default="")
    lease_expires_at = models.DateTimeField(null=True, blank=True)
    done_payload = models.JSONField(null=True, blank=True)
    error = models.TextField(blank=True, default="")
    created_at = models.DateTimeField(auto_now_add=True)
    assigned_at = models.DateTimeField(null=True, blank=True)
    started_at = models.DateTimeField(null=True, blank=True)
    ended_at = models.DateTimeField(null=True, blank=True)

    class Meta:
        db_table = "agent_run"
        ordering = ("-created_at",)
        indexes = [
            models.Index(fields=["runner", "status"]),
            models.Index(fields=["owner", "status"]),
            models.Index(fields=["workspace", "status"]),
            models.Index(fields=["work_item", "status"]),
            models.Index(fields=["pod", "status"], name="agent_run_pod_status_idx"),
            models.Index(
                fields=["created_by", "status"], name="agent_run_created_status_idx"
            ),
        ]

    def save(self, *args, **kwargs):
        # Auto-resolve pod when omitted. The view layer (orchestration
        # code that creates AgentRun rows) is the canonical place to
        # set this; the fallbacks below keep direct-ORM callers
        # (tests, management commands) honest. Resolution order:
        # 1. work_item.project's default pod (post-refactor canonical).
        # 2. Single-project workspace's only project's default pod
        #    (back-compat for tests / single-project installs).
        # The pre-refactor "workspace default pod" lookup is gone — see
        # §8 of the new_pod_project_relationship design.
        if self.pod_id is None and self.work_item_id is not None:
            project_id = (
                self.work_item.project_id
                if hasattr(self.work_item, "project_id")
                else None
            )
            if project_id is not None:
                default = Pod.default_for_project_id(project_id)
                if default is not None:
                    self.pod = default
        if self.pod_id is None and self.workspace_id is not None:
            from pi_dash.db.models.project import Project

            project_ids = list(
                Project.objects.filter(workspace_id=self.workspace_id)
                .values_list("id", flat=True)[:2]
            )
            if len(project_ids) == 1:
                default = Pod.default_for_project_id(project_ids[0])
                if default is not None:
                    self.pod = default
        # Back-compat: legacy call sites that set `owner` but not `created_by`
        # (to be audited and removed in Phase 3). Mirror owner into created_by
        # so the NOT NULL constraint holds. The design's interpretation for
        # historical rows is that owner == created_by under the old model.
        if self.created_by_id is None and self.owner_id is not None:
            self.created_by_id = self.owner_id
        super().save(*args, **kwargs)

    @property
    def is_terminal(self) -> bool:
        return self.status in {
            AgentRunStatus.COMPLETED,
            AgentRunStatus.FAILED,
            AgentRunStatus.CANCELLED,
            AgentRunStatus.BLOCKED,
        }

    @property
    def is_active(self) -> bool:
        """Active runs occupy the single-active-run slot per issue."""
        return self.status in {
            AgentRunStatus.QUEUED,
            AgentRunStatus.ASSIGNED,
            AgentRunStatus.RUNNING,
            AgentRunStatus.AWAITING_APPROVAL,
            AgentRunStatus.AWAITING_REAUTH,
        }


class AgentRunEvent(models.Model):
    """Append-only transcript of events streamed from the runner."""

    id = models.BigAutoField(primary_key=True)
    agent_run = models.ForeignKey(
        AgentRun, on_delete=models.CASCADE, related_name="events"
    )
    seq = models.PositiveIntegerField()
    kind = models.CharField(max_length=64)
    payload = models.JSONField(default=dict, blank=True)
    created_at = models.DateTimeField(auto_now_add=True)

    class Meta:
        db_table = "agent_run_event"
        unique_together = [("agent_run", "seq")]
        ordering = ("agent_run", "seq")


class ApprovalRequest(models.Model):
    id = models.UUIDField(
        primary_key=True, default=uuid.uuid4, editable=False, db_index=True
    )
    agent_run = models.ForeignKey(
        AgentRun, on_delete=models.CASCADE, related_name="approvals"
    )
    kind = models.CharField(max_length=24, choices=ApprovalKind.choices)
    payload = models.JSONField(default=dict, blank=True)
    reason = models.TextField(blank=True, default="")
    status = models.CharField(
        max_length=16,
        choices=ApprovalStatus.choices,
        default=ApprovalStatus.PENDING,
        db_index=True,
    )
    decision_source = models.CharField(max_length=16, blank=True, default="")
    decided_by = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        null=True,
        blank=True,
        on_delete=models.SET_NULL,
        related_name="runner_approvals_decided",
    )
    requested_at = models.DateTimeField(auto_now_add=True)
    expires_at = models.DateTimeField(null=True, blank=True)
    decided_at = models.DateTimeField(null=True, blank=True)

    class Meta:
        db_table = "agent_run_approval"
        ordering = ("-requested_at",)
        indexes = [models.Index(fields=["agent_run", "status"])]


class RunnerLiveState(models.Model):
    """Volatile per-runner observability snapshot for the active agent run.

    Holds the descriptive scalars the runner emits on every poll
    (``last_event_at``, ``last_event_kind``, ``last_event_summary``,
    ``agent_pid``, ``agent_subprocess_alive``, ``approvals_pending``,
    streaming token counts, ``turn_count``). All fields are nullable;
    NULL is the canonical "unknown" sentinel for both the watchdog and
    the UI. See ``.ai_design/runner_agent_bridge/design.md`` §4.5.1.

    Authoritative run state stays on :class:`AgentRun`; this row is
    overwritten / cleared on each ``observed_run_id`` change. The
    watchdog (``reconcile_stalled_runs``) only acts when this row's
    ``observed_run_id`` matches an active ``AgentRun.id`` and
    ``updated_at`` is fresh.
    """

    runner = models.OneToOneField(
        Runner,
        on_delete=models.CASCADE,
        primary_key=True,
        related_name="live_state",
    )
    # The run this snapshot describes. NULL when the runner is idle.
    # The watchdog only acts when this matches a running AgentRun's id —
    # that join condition is what makes the snapshot unambiguously about
    # the run we'd otherwise fail.
    observed_run_id = models.UUIDField(null=True, blank=True)
    last_event_at = models.DateTimeField(null=True, blank=True)
    last_event_kind = models.CharField(max_length=64, null=True, blank=True)
    last_event_summary = models.CharField(max_length=200, null=True, blank=True)
    agent_pid = models.PositiveIntegerField(null=True, blank=True)
    agent_subprocess_alive = models.BooleanField(null=True, blank=True)
    # PositiveIntegerField (Postgres INTEGER, max 2_147_483_647), not
    # SmallInteger: the runner serialises this as u32. Real-world counts
    # are tiny so the int32 ceiling is unreachable in practice; the
    # u32::MAX (4_294_967_295) saturating sentinel the runner uses on
    # `usize → u32` conversion would still overflow this column, but
    # producing approvals_pending > 4 billion is not a realistic path.
    approvals_pending = models.PositiveIntegerField(null=True, blank=True)
    input_tokens = models.BigIntegerField(null=True, blank=True)
    output_tokens = models.BigIntegerField(null=True, blank=True)
    total_tokens = models.BigIntegerField(null=True, blank=True)
    turn_count = models.PositiveIntegerField(null=True, blank=True)
    updated_at = models.DateTimeField(auto_now=True)

    class Meta:
        db_table = "runner_live_state"
        indexes = [
            # Watchdog query in reconcile_stalled_runs filters on
            # observed_run_id (run-id match), updated_at (snapshot fresh),
            # and last_event_at (agent activity stale).
            models.Index(
                fields=["observed_run_id", "updated_at", "last_event_at"],
                name="runner_live_watchdog_idx",
            ),
        ]

    def __str__(self) -> str:
        return f"RunnerLiveState(runner={self.runner_id} observed_run_id={self.observed_run_id})"
