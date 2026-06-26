# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import models

from pi_dash.db.models.base import BaseModel
from pi_dash.db.models.project import ProjectBaseModel


class GitProviderAccount(BaseModel):
    class Provider(models.TextChoices):
        GITHUB = "github", "GitHub"
        GITLAB = "gitlab", "GitLab"

    class AuthType(models.TextChoices):
        GITHUB_APP = "github_app", "GitHub App"
        PAT = "pat", "Personal Access Token"
        OAUTH = "oauth", "OAuth"
        GROUP_TOKEN = "group_token", "Group Token"
        PROJECT_TOKEN = "project_token", "Project Token"

    class Status(models.TextChoices):
        CONNECTED = "connected", "Connected"
        DEGRADED = "degraded", "Degraded"
        REVOKED = "revoked", "Revoked"
        ERROR = "error", "Error"

    workspace = models.ForeignKey("db.Workspace", related_name="git_provider_accounts", on_delete=models.CASCADE)
    provider = models.CharField(max_length=32, choices=Provider.choices)
    host_url = models.URLField(max_length=500)
    auth_type = models.CharField(max_length=32, choices=AuthType.choices)
    external_account_id = models.CharField(max_length=255, blank=True, default="")
    external_account_login = models.CharField(max_length=255, blank=True, default="")
    display_name = models.CharField(max_length=255, blank=True, default="")
    capabilities = models.JSONField(default=dict)
    credential_config = models.JSONField(default=dict)
    workspace_integration = models.ForeignKey(
        "db.WorkspaceIntegration",
        related_name="git_provider_accounts",
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
    )
    status = models.CharField(max_length=16, choices=Status.choices, default=Status.CONNECTED)
    verified_at = models.DateTimeField(null=True, blank=True)
    last_check_error = models.TextField(blank=True, default="")
    metadata = models.JSONField(default=dict)

    def __str__(self):
        label = self.display_name or self.external_account_login or self.external_account_id or self.host_url
        return f"{self.provider}:{label} <{self.workspace_id}>"

    class Meta:
        db_table = "git_provider_accounts"
        verbose_name = "Git Provider Account"
        verbose_name_plural = "Git Provider Accounts"
        ordering = ("-created_at",)
        indexes = [
            models.Index(fields=["workspace", "provider", "host_url"], name="git_pa_ws_provider_host_idx"),
            models.Index(fields=["provider", "host_url", "external_account_id"], name="git_pa_provider_ext_idx"),
        ]


class GitRepository(BaseModel):
    class Provider(models.TextChoices):
        GITHUB = "github", "GitHub"
        GITLAB = "gitlab", "GitLab"

    provider = models.CharField(max_length=32, choices=Provider.choices)
    host_url = models.URLField(max_length=500)
    external_id = models.CharField(max_length=255, blank=True, default="")
    namespace = models.CharField(max_length=500)
    name = models.CharField(max_length=500)
    full_name = models.CharField(max_length=1000)
    web_url = models.URLField(max_length=1000, blank=True, default="")
    clone_url_http = models.CharField(max_length=1000, blank=True, default="")
    clone_url_ssh = models.CharField(max_length=1000, blank=True, default="")
    default_branch = models.CharField(max_length=255, blank=True, default="")
    is_private = models.BooleanField(default=False)
    metadata = models.JSONField(default=dict)

    def __str__(self):
        return f"{self.provider}:{self.full_name}"

    class Meta:
        db_table = "git_repositories"
        verbose_name = "Git Repository"
        verbose_name_plural = "Git Repositories"
        ordering = ("-created_at",)
        constraints = [
            models.UniqueConstraint(
                fields=["provider", "host_url", "external_id"],
                condition=models.Q(deleted_at__isnull=True) & ~models.Q(external_id=""),
                name="git_repo_uniq_ext_active",
            ),
            models.UniqueConstraint(
                fields=["provider", "host_url", "full_name"],
                condition=models.Q(deleted_at__isnull=True),
                name="git_repo_uniq_full_active",
            ),
        ]
        indexes = [
            models.Index(fields=["provider", "host_url", "full_name"], name="git_repo_provider_full_idx"),
        ]


class GitRepositoryBinding(ProjectBaseModel):
    class CloneAuthMode(models.TextChoices):
        PUBLIC = "public", "Public"
        RUNNER_MANAGED = "runner_managed", "Runner Managed"
        MANAGED_EPHEMERAL = "managed_ephemeral", "Managed Ephemeral"

    repository = models.ForeignKey("db.GitRepository", related_name="bindings", on_delete=models.CASCADE)
    provider_account = models.ForeignKey(
        "db.GitProviderAccount",
        related_name="repository_bindings",
        on_delete=models.PROTECT,
    )
    actor = models.ForeignKey("db.User", related_name="git_repository_bindings", on_delete=models.CASCADE)
    is_sync_enabled = models.BooleanField(default=False)
    clone_auth_mode = models.CharField(
        max_length=32,
        choices=CloneAuthMode.choices,
        default=CloneAuthMode.RUNNER_MANAGED,
    )
    last_synced_at = models.DateTimeField(null=True, blank=True)
    last_sync_error = models.TextField(blank=True, default="")
    metadata = models.JSONField(default=dict)

    def __str__(self):
        return f"{self.repository.full_name} <{self.project.name}>"

    class Meta:
        db_table = "git_repository_bindings"
        verbose_name = "Git Repository Binding"
        verbose_name_plural = "Git Repository Bindings"
        ordering = ("-created_at",)
        constraints = [
            models.UniqueConstraint(
                fields=["project"],
                condition=models.Q(deleted_at__isnull=True),
                name="git_bind_uniq_project_active",
            ),
        ]
        indexes = [
            models.Index(fields=["workspace", "project"], name="git_bind_ws_project_idx"),
            models.Index(fields=["repository", "provider_account"], name="git_bind_repo_acct_idx"),
        ]


class GitIssueSync(ProjectBaseModel):
    binding = models.ForeignKey("db.GitRepositoryBinding", related_name="issue_syncs", on_delete=models.CASCADE)
    issue = models.ForeignKey("db.Issue", related_name="git_syncs", on_delete=models.CASCADE)
    provider = models.CharField(max_length=32)
    external_id = models.CharField(max_length=255, blank=True, default="")
    external_iid = models.CharField(max_length=255)
    web_url = models.URLField(max_length=1000, blank=True, default="")
    remote_state = models.CharField(max_length=64, blank=True, default="")
    remote_created_at = models.DateTimeField(null=True, blank=True)
    remote_updated_at = models.DateTimeField(null=True, blank=True)
    metadata = models.JSONField(default=dict)

    def __str__(self):
        return f"{self.provider}:{self.external_iid} <{self.issue_id}>"

    class Meta:
        db_table = "git_issue_syncs"
        verbose_name = "Git Issue Sync"
        verbose_name_plural = "Git Issue Syncs"
        ordering = ("-created_at",)
        constraints = [
            models.UniqueConstraint(
                fields=["binding", "external_iid"],
                condition=models.Q(deleted_at__isnull=True),
                name="git_issue_uniq_iid_active",
            ),
            models.UniqueConstraint(
                fields=["binding", "issue"],
                condition=models.Q(deleted_at__isnull=True),
                name="git_issue_uniq_issue_active",
            ),
        ]
        indexes = [
            models.Index(fields=["issue"], name="git_issue_sync_issue_idx"),
            models.Index(fields=["provider", "external_iid"], name="git_issue_provider_iid_idx"),
        ]


class GitCommentSync(ProjectBaseModel):
    issue_sync = models.ForeignKey("db.GitIssueSync", related_name="comment_syncs", on_delete=models.CASCADE)
    comment = models.ForeignKey("db.IssueComment", related_name="git_comment_syncs", on_delete=models.CASCADE)
    provider = models.CharField(max_length=32)
    external_id = models.CharField(max_length=255)
    remote_created_at = models.DateTimeField(null=True, blank=True)
    remote_updated_at = models.DateTimeField(null=True, blank=True)
    metadata = models.JSONField(default=dict)

    def __str__(self):
        return f"{self.provider}:{self.external_id} <{self.comment_id}>"

    class Meta:
        db_table = "git_comment_syncs"
        verbose_name = "Git Comment Sync"
        verbose_name_plural = "Git Comment Syncs"
        ordering = ("-created_at",)
        constraints = [
            models.UniqueConstraint(
                fields=["issue_sync", "external_id"],
                condition=models.Q(deleted_at__isnull=True),
                name="git_comment_uniq_ext_active",
            ),
            models.UniqueConstraint(
                fields=["issue_sync", "comment"],
                condition=models.Q(deleted_at__isnull=True),
                name="git_comment_uniq_comment_act",
            ),
        ]
        indexes = [
            models.Index(fields=["comment"], name="git_comment_comment_idx"),
        ]


class GitCodeReviewLink(ProjectBaseModel):
    class State(models.TextChoices):
        OPEN = "open", "Open"
        CLOSED = "closed", "Closed"
        MERGED = "merged", "Merged"

    issue = models.ForeignKey("db.Issue", related_name="git_code_reviews", on_delete=models.CASCADE)
    provider = models.CharField(max_length=32)
    host_url = models.URLField(max_length=500)
    namespace = models.CharField(max_length=500)
    repo_name = models.CharField(max_length=500)
    repo_external_id = models.CharField(max_length=255, blank=True, default="")
    external_id = models.CharField(max_length=255, blank=True, default="")
    external_iid = models.CharField(max_length=255)
    url = models.URLField(max_length=1000)
    title = models.CharField(max_length=500, blank=True, default="")
    state = models.CharField(max_length=16, choices=State.choices, default=State.OPEN)
    merged = models.BooleanField(default=False)
    draft = models.BooleanField(default=False)
    remote_updated_at = models.DateTimeField(null=True, blank=True)
    metadata = models.JSONField(default=dict)

    def __str__(self):
        return f"{self.provider}:{self.namespace}/{self.repo_name}!{self.external_iid} <{self.issue_id}>"

    class Meta:
        db_table = "git_code_review_links"
        verbose_name = "Git Code Review Link"
        verbose_name_plural = "Git Code Review Links"
        ordering = ("-created_at",)
        constraints = [
            models.UniqueConstraint(
                fields=["provider", "host_url", "repo_external_id", "external_iid"],
                condition=models.Q(deleted_at__isnull=True) & ~models.Q(repo_external_id=""),
                name="git_cr_uniq_repo_iid_active",
            ),
            models.UniqueConstraint(
                fields=["provider", "host_url", "namespace", "repo_name", "external_iid"],
                condition=models.Q(deleted_at__isnull=True) & models.Q(repo_external_id=""),
                name="git_cr_uniq_path_iid_active",
            ),
        ]
        indexes = [
            models.Index(fields=["issue"], name="git_cr_issue_idx"),
            models.Index(fields=["provider", "host_url", "namespace", "repo_name"], name="git_cr_provider_repo_idx"),
        ]


class GitWebhookDelivery(BaseModel):
    class Status(models.TextChoices):
        RECEIVED = "received", "Received"
        PROCESSED = "processed", "Processed"
        FAILED = "failed", "Failed"
        SKIPPED = "skipped", "Skipped"

    provider = models.CharField(max_length=32)
    host_url = models.URLField(max_length=500, blank=True, default="")
    delivery_id = models.CharField(max_length=255, db_index=True)
    event = models.CharField(max_length=100)
    action = models.CharField(max_length=100, blank=True, default="")
    repository = models.ForeignKey("db.GitRepository", related_name="webhook_deliveries", null=True, blank=True, on_delete=models.SET_NULL)
    raw_headers = models.JSONField(default=dict)
    payload = models.JSONField(default=dict)
    status = models.CharField(max_length=16, choices=Status.choices, default=Status.RECEIVED)
    received_at = models.DateTimeField(auto_now_add=True)
    processed_at = models.DateTimeField(null=True, blank=True)
    error = models.TextField(blank=True, default="")
    metadata = models.JSONField(default=dict)

    def __str__(self):
        return f"{self.provider}:{self.event}:{self.delivery_id}"

    class Meta:
        db_table = "git_webhook_deliveries"
        verbose_name = "Git Webhook Delivery"
        verbose_name_plural = "Git Webhook Deliveries"
        ordering = ("-received_at",)
        constraints = [
            models.UniqueConstraint(
                fields=["provider", "host_url", "delivery_id"],
                condition=models.Q(deleted_at__isnull=True),
                name="git_wh_delivery_uniq_active",
            ),
        ]


class GitWebhookRegistration(BaseModel):
    repository = models.ForeignKey("db.GitRepository", related_name="webhook_registrations", on_delete=models.CASCADE)
    provider_account = models.ForeignKey(
        "db.GitProviderAccount",
        related_name="webhook_registrations",
        on_delete=models.CASCADE,
    )
    provider_hook_id = models.CharField(max_length=255, blank=True, default="")
    events = models.JSONField(default=list)
    secret_ref = models.CharField(max_length=255, blank=True, default="")
    last_verified_at = models.DateTimeField(null=True, blank=True)
    last_check_error = models.TextField(blank=True, default="")
    metadata = models.JSONField(default=dict)

    def __str__(self):
        return f"{self.provider_account.provider}:{self.repository.full_name}:{self.provider_hook_id}"

    class Meta:
        db_table = "git_webhook_registrations"
        verbose_name = "Git Webhook Registration"
        verbose_name_plural = "Git Webhook Registrations"
        ordering = ("-created_at",)
        constraints = [
            models.UniqueConstraint(
                fields=["repository", "provider_account"],
                condition=models.Q(deleted_at__isnull=True),
                name="git_wh_reg_uniq_repo_acct",
            ),
        ]
