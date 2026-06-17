# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# Python imports

# Django imports
from django.db import models

# Module imports
from pi_dash.db.models.base import BaseModel
from pi_dash.db.models.project import ProjectBaseModel


class GithubRepository(ProjectBaseModel):
    name = models.CharField(max_length=500)
    url = models.URLField(null=True)
    config = models.JSONField(default=dict)
    repository_id = models.BigIntegerField()
    owner = models.CharField(max_length=500)

    def __str__(self):
        """Return the repo name"""
        return f"{self.name}"

    class Meta:
        verbose_name = "Repository"
        verbose_name_plural = "Repositories"
        db_table = "github_repositories"
        ordering = ("-created_at",)


class GithubRepositorySync(ProjectBaseModel):
    # `ForeignKey`, not `OneToOneField`: the latter implicitly enforces
    # a unique constraint on `repository_id`, which (just like the old
    # `unique_together = [project, repository]` dropped in migration
    # 0128) blocks rebinding a project to a previously-soft-deleted
    # repo. The active-binding invariant is already covered by
    # `github_repository_sync_unique_per_project_when_active` below.
    repository = models.ForeignKey("db.GithubRepository", on_delete=models.CASCADE, related_name="syncs")
    credentials = models.JSONField(default=dict)
    # Bot user
    actor = models.ForeignKey("db.User", related_name="user_syncs", on_delete=models.CASCADE)
    workspace_integration = models.ForeignKey(
        "db.WorkspaceIntegration", related_name="github_syncs", on_delete=models.CASCADE
    )
    label = models.ForeignKey("db.Label", on_delete=models.SET_NULL, null=True, related_name="repo_syncs")
    # Sync operational state — see .ai_design/github_sync/design.md §5.
    is_sync_enabled = models.BooleanField(default=False)
    last_synced_at = models.DateTimeField(null=True, blank=True)
    last_sync_error = models.TextField(blank=True, default="")

    def __str__(self):
        """Return the repo sync"""
        return f"{self.repository.name} <{self.project.name}>"

    class Meta:
        # No `unique_together = [project, repository]` — the constraint
        # below already enforces the operational invariant (one active
        # binding per project) and is correctly filtered on `deleted_at`
        # so unbind+rebind to the same repo doesn't collide with the
        # soft-deleted row. See migration 0128 for context.
        constraints = [
            models.UniqueConstraint(
                fields=["project"],
                condition=models.Q(deleted_at__isnull=True),
                name="github_repository_sync_unique_per_project_when_active",
            ),
        ]
        verbose_name = "Github Repository Sync"
        verbose_name_plural = "Github Repository Syncs"
        db_table = "github_repository_syncs"
        ordering = ("-created_at",)


class GithubIssueSync(ProjectBaseModel):
    repo_issue_id = models.BigIntegerField()
    github_issue_id = models.BigIntegerField()
    issue_url = models.URLField(blank=False)
    issue = models.ForeignKey("db.Issue", related_name="github_syncs", on_delete=models.CASCADE)
    repository_sync = models.ForeignKey("db.GithubRepositorySync", related_name="issue_syncs", on_delete=models.CASCADE)
    # See .ai_design/github_sync/design.md §5: stores
    #   - completion_comment_id (idempotency for §6.5)
    #   - upstream_gone_at (deletion/closure flag, §6.3.1)
    #   - github_user_login (author identity)
    metadata = models.JSONField(default=dict)
    # GitHub-side timestamps. Kept here (not on Issue) because TimeAuditModel
    # forces auto_now_add/auto_now on Issue.created_at/updated_at.
    gh_issue_created_at = models.DateTimeField(null=True, blank=True)
    gh_issue_updated_at = models.DateTimeField(null=True, blank=True)

    def __str__(self):
        """Return the github issue sync"""
        return f"{self.repository_sync.repository.name}-{self.project.name}-{self.issue.name}"

    class Meta:
        unique_together = ["repository_sync", "issue"]
        verbose_name = "Github Issue Sync"
        verbose_name_plural = "Github Issue Syncs"
        db_table = "github_issue_syncs"
        ordering = ("-created_at",)


class GithubCommentSync(ProjectBaseModel):
    repo_comment_id = models.BigIntegerField()
    comment = models.ForeignKey("db.IssueComment", related_name="comment_syncs", on_delete=models.CASCADE)
    issue_sync = models.ForeignKey("db.GithubIssueSync", related_name="comment_syncs", on_delete=models.CASCADE)

    def __str__(self):
        """Return the github issue sync"""
        return f"{self.comment.id}"

    class Meta:
        unique_together = ["issue_sync", "comment"]
        verbose_name = "Github Comment Sync"
        verbose_name_plural = "Github Comment Syncs"
        db_table = "github_comment_syncs"
        ordering = ("-created_at",)


class GithubAppInstallation(BaseModel):
    class AccountType(models.TextChoices):
        USER = "User", "User"
        ORGANIZATION = "Organization", "Organization"
        UNKNOWN = "Unknown", "Unknown"

    class RepositorySelection(models.TextChoices):
        ALL = "all", "All"
        SELECTED = "selected", "Selected"

    workspace_integration = models.OneToOneField(
        "db.WorkspaceIntegration",
        related_name="github_app_installation",
        on_delete=models.CASCADE,
    )
    installation_id = models.BigIntegerField(unique=True, db_index=True)
    account_login = models.CharField(max_length=255, blank=True, default="")
    account_type = models.CharField(max_length=32, choices=AccountType.choices, default=AccountType.UNKNOWN)
    repository_selection = models.CharField(
        max_length=16,
        choices=RepositorySelection.choices,
        default=RepositorySelection.SELECTED,
    )
    repository_count = models.PositiveIntegerField(default=0)
    permissions = models.JSONField(default=dict)
    events = models.JSONField(default=list)
    installed_at = models.DateTimeField(null=True, blank=True)
    suspended_at = models.DateTimeField(null=True, blank=True)
    verified_at = models.DateTimeField(null=True, blank=True)
    last_checked_at = models.DateTimeField(null=True, blank=True)
    last_check_error = models.TextField(blank=True, default="")

    def __str__(self):
        return f"{self.account_login or self.installation_id} <{self.workspace_integration.workspace.name}>"

    class Meta:
        verbose_name = "Github App Installation"
        verbose_name_plural = "Github App Installations"
        db_table = "github_app_installations"
        ordering = ("-created_at",)


class GithubWebhookDelivery(BaseModel):
    class Status(models.TextChoices):
        RECEIVED = "received", "Received"
        PROCESSED = "processed", "Processed"
        FAILED = "failed", "Failed"
        SKIPPED = "skipped", "Skipped"

    delivery_id = models.UUIDField(unique=True, db_index=True)
    event = models.CharField(max_length=100)
    action = models.CharField(max_length=100, blank=True, default="")
    installation_id = models.BigIntegerField(null=True, blank=True, db_index=True)
    payload = models.JSONField(default=dict)
    status = models.CharField(max_length=16, choices=Status.choices, default=Status.RECEIVED)
    received_at = models.DateTimeField(auto_now_add=True)
    processed_at = models.DateTimeField(null=True, blank=True)
    error = models.TextField(blank=True, default="")

    def __str__(self):
        return f"{self.event}:{self.delivery_id}"

    class Meta:
        verbose_name = "Github Webhook Delivery"
        verbose_name_plural = "Github Webhook Deliveries"
        db_table = "github_webhook_deliveries"
        ordering = ("-received_at",)


class GithubAppInstallSession(BaseModel):
    class Status(models.TextChoices):
        STARTED = "started", "Started"
        COMPLETED = "completed", "Completed"
        EXPIRED = "expired", "Expired"
        FAILED = "failed", "Failed"

    state = models.CharField(max_length=128, unique=True, db_index=True)
    workspace = models.ForeignKey("db.Workspace", related_name="github_app_install_sessions", on_delete=models.CASCADE)
    actor = models.ForeignKey("db.User", related_name="github_app_install_sessions", on_delete=models.CASCADE)
    installation_id = models.BigIntegerField(null=True, blank=True)
    account_login = models.CharField(max_length=255, blank=True, default="")
    status = models.CharField(max_length=16, choices=Status.choices, default=Status.STARTED, db_index=True)
    expires_at = models.DateTimeField(db_index=True)
    completed_at = models.DateTimeField(null=True, blank=True)
    error = models.TextField(blank=True, default="")

    def __str__(self):
        return f"{self.workspace.slug}:{self.status}:{self.state}"

    class Meta:
        verbose_name = "Github App Install Session"
        verbose_name_plural = "Github App Install Sessions"
        db_table = "github_app_install_sessions"
        ordering = ("-created_at",)
