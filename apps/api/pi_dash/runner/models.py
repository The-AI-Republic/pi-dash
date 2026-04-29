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
        # both. clean() rejects mismatches.
        if self.project_id is not None and self.workspace_id is None:
            self.workspace_id = self.project.workspace_id
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
    """A physical dev machine that can execute AgentRuns."""

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
    # Runner authenticates over WS with a bearer token; we store only its hash.
    credential_hash = models.CharField(max_length=128, db_index=True)
    credential_fingerprint = models.CharField(max_length=16)
    # Machine token (multi-runner). When present, the daemon hosting this
    # runner authenticates as the token rather than presenting the runner's
    # own credential. NULL on legacy v1 installs that haven't migrated yet
    # and on the transitional period before phase 2 ships in production.
    # See ``design.md`` §5.1.
    machine_token = models.ForeignKey(
        "MachineToken",
        null=True,
        blank=True,
        on_delete=models.SET_NULL,
        related_name="runners",
    )
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
        # Pod must be set explicitly at runner creation. The earlier
        # auto-resolution to "the workspace's default pod" is gone:
        # pods are now project-scoped, and the runner registration
        # endpoints (register/, register-under-token/) resolve a
        # specific project's pod before instantiating the Runner.
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

    def revoke(self) -> None:
        """Mark the runner revoked and synchronously cancel any in-flight runs.

        See ``.ai_design/issue_runner/design.md`` §7.5. Without this, a
        force-closed runner leaves ``ASSIGNED``/``RUNNING`` rows stranded
        because ``consumers._finalize_run`` only triggers on runner-sent
        completion messages that may never arrive after revocation.

        After the transaction commits, the affected pods are re-drained so
        queued work (if any) attempts to move to remaining online runners in
        the same pod.
        """
        # Imports deferred to avoid a circular dependency (matcher imports
        # Runner model; Runner.revoke calls into matcher).
        from django.db import transaction
        from pi_dash.runner.services.matcher import (
            NON_TERMINAL_STATUSES,
            drain_pod_by_id,
        )

        affected_pod_ids: set = set()
        with transaction.atomic():
            now = timezone.now()
            Runner.objects.filter(pk=self.pk).update(
                status=RunnerStatus.REVOKED,
                revoked_at=now,
            )
            # Refresh in-memory instance so callers see the new status/timestamp
            # without an extra query on their side.
            self.status = RunnerStatus.REVOKED
            self.revoked_at = now

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

            # Pinned QUEUED runs (follow-ups waiting for this runner) don't
            # get cancelled — they still represent legitimate user intent.
            # Drop the pin so they flow back into the pod's general queue
            # and any remaining online runner can take them with a fresh
            # session. See §5.7 of .ai_design/issue_run_improve/design.md.
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

        # Refire drain for every affected pod once the transaction commits.
        # Remaining online runners (if any) in the same pod may now pick up
        # queued work.
        for pod_id in affected_pod_ids:
            transaction.on_commit(lambda pid=pod_id: drain_pod_by_id(pid))


MAX_RUNNERS_PER_MACHINE = 50


class MachineToken(models.Model):
    """Long-lived machine credential — authenticates one dev-machine daemon
    over WebSocket and authorises it to act as N runners under it.

    See ``.ai_design/n_runners_in_same_machine/design.md`` §5.1. Each
    daemon process holds exactly one MachineToken; the token's ``owns``
    set (resolved via ``Runner.machine_token`` FK) is the list of runners
    the daemon may bring online with ``Hello { runner_id }``.

    Surfaced in the Pi Dash UI as a "connection" with a user-supplied
    title; revoking the token tears down the daemon's connection on next
    auth check (synchronous via connection-scoped Revoke when the WS is
    up; on next reconnect via 401 otherwise) and cascades to all owned
    runners going offline. Tokens are not rotatable — only create/revoke
    per decisions.md Q8.
    """

    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    workspace = models.ForeignKey(
        "db.Workspace",
        on_delete=models.CASCADE,
        related_name="machine_tokens",
    )
    created_by = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        on_delete=models.CASCADE,
        related_name="machine_tokens",
    )
    # User-supplied label, shown in the connections section of the UI.
    title = models.CharField(max_length=128)
    # Bearer secret hash. Plaintext is shown to the user once at creation
    # and never persisted server-side.
    secret_hash = models.CharField(max_length=128, db_index=True)
    secret_fingerprint = models.CharField(max_length=16)
    last_seen_at = models.DateTimeField(null=True, blank=True)
    created_at = models.DateTimeField(auto_now_add=True)
    revoked_at = models.DateTimeField(null=True, blank=True, db_index=True)

    class Meta:
        db_table = "machine_token"
        ordering = ("-created_at",)
        indexes = [
            models.Index(fields=["workspace", "revoked_at"]),
            models.Index(fields=["created_by", "revoked_at"]),
        ]

    def __str__(self) -> str:
        return f"{self.title} (ws={self.workspace_id})"

    def is_active(self) -> bool:
        return self.revoked_at is None

    def revoke(self) -> None:
        """Mark the token revoked and cascade to its owned runners.

        Owned runners are revoked individually (which cancels their
        in-flight runs via ``Runner.revoke``) and we then push a
        connection-scoped ``Revoke`` frame to the consumer so the
        daemon shuts down cleanly via its supervisor's
        ``state.shutdown()`` path. In token mode one consumer is joined
        to N runner groups on one connection, so a single
        ``send_token_revoke`` call reaches the consumer once — that's
        all we need; the daemon then exits and won't reconnect (auth
        would fail anyway since ``revoked_at`` is non-null).

        If the WS is already down, the message is dropped and the next
        reconnect fails the auth check.
        """
        from django.db import transaction

        from pi_dash.runner.services.pubsub import (
            close_runner_session,
            send_token_revoke,
        )

        if self.revoked_at is not None:
            return
        owned_runner_ids: list = []
        with transaction.atomic():
            self.revoked_at = timezone.now()
            self.save(update_fields=["revoked_at"])
            # Cascade — revoke each owned runner so its in-flight runs
            # are cancelled and any pinned queued runs unpinned. Capture
            # ids for the post-commit notification step.
            for runner in self.runners.filter(revoked_at__isnull=True):
                runner.revoke()
                owned_runner_ids.append(runner.id)

        # After the DB transaction commits, tell the daemon (if connected)
        # to shut down via a wire-level Revoke frame. Broadcast to every
        # owned runner group rather than assuming any one runner is
        # definitely online on the live connection — some may be stale,
        # offline, or no longer Hello-authorised on this daemon. Defence
        # in depth: also force-close every group, so a consumer that
        # drops the Revoke for any reason still loses the socket.
        for runner_id in owned_runner_ids:
            send_token_revoke(runner_id, reason="token revoked")
        for runner_id in owned_runner_ids:
            close_runner_session(runner_id)


class RunnerRegistrationToken(models.Model):
    """Short-lived, single-use token used to pair a new runner with the cloud."""

    id = models.UUIDField(
        primary_key=True, default=uuid.uuid4, editable=False, db_index=True
    )
    workspace = models.ForeignKey(
        "db.Workspace",
        on_delete=models.CASCADE,
        related_name="runner_registration_tokens",
    )
    created_by = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        on_delete=models.CASCADE,
        related_name="runner_registration_tokens",
    )
    token_hash = models.CharField(max_length=128, unique=True)
    label = models.CharField(max_length=128, blank=True, default="")
    expires_at = models.DateTimeField()
    consumed_at = models.DateTimeField(null=True, blank=True)
    consumed_by_runner = models.ForeignKey(
        Runner,
        null=True,
        blank=True,
        on_delete=models.SET_NULL,
        related_name="consumed_registration_tokens",
    )
    created_at = models.DateTimeField(auto_now_add=True)

    class Meta:
        db_table = "runner_registration_token"
        ordering = ("-created_at",)
        indexes = [models.Index(fields=["workspace", "consumed_at"])]

    def is_valid(self) -> bool:
        return self.consumed_at is None and self.expires_at > timezone.now()


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
        # Auto-resolve pod to the project's default pod when omitted.
        # The view layer (orchestration code that creates AgentRun rows)
        # is the canonical place to set the pod; this fallback keeps
        # direct-ORM callers (tests, management commands) honest as
        # long as the run's work_item has a project. The previous
        # workspace-default lookup is gone — see §8 of the
        # new_pod_project_relationship design.
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
