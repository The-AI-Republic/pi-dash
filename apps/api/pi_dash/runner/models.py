# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import uuid

from django.conf import settings
from django.db import models
from django.utils import timezone


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
    name = models.CharField(max_length=128)
    # Runner authenticates over WS with a bearer token; we store only its hash.
    credential_hash = models.CharField(max_length=128, db_index=True)
    credential_fingerprint = models.CharField(max_length=16)
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
        indexes = [
            models.Index(fields=["owner", "status"]),
            models.Index(fields=["workspace", "status"]),
        ]

    def __str__(self) -> str:
        return f"{self.name} ({self.owner_id})"

    def mark_heartbeat(self) -> None:
        self.last_heartbeat_at = timezone.now()
        self.save(update_fields=["last_heartbeat_at"])

    def revoke(self) -> None:
        self.status = RunnerStatus.REVOKED
        self.revoked_at = timezone.now()
        self.save(update_fields=["status", "revoked_at"])


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
    owner = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        on_delete=models.CASCADE,
        related_name="agent_runs",
    )
    runner = models.ForeignKey(
        Runner,
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
        related_name="agent_runs",
    )
    work_item = models.ForeignKey(
        "db.Issue",
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
        related_name="agent_runs",
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
        ]

    @property
    def is_terminal(self) -> bool:
        return self.status in {
            AgentRunStatus.COMPLETED,
            AgentRunStatus.FAILED,
            AgentRunStatus.CANCELLED,
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
