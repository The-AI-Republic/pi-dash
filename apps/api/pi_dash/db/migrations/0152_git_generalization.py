# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


GITHUB_HOST = "https://github.com"


def backfill_github_to_generic(apps, schema_editor):
    WorkspaceIntegration = apps.get_model("db", "WorkspaceIntegration")
    GitProviderAccount = apps.get_model("db", "GitProviderAccount")
    GitRepository = apps.get_model("db", "GitRepository")
    GitRepositoryBinding = apps.get_model("db", "GitRepositoryBinding")
    GitIssueSync = apps.get_model("db", "GitIssueSync")
    GitCommentSync = apps.get_model("db", "GitCommentSync")
    GitCodeReviewLink = apps.get_model("db", "GitCodeReviewLink")
    GitWebhookDelivery = apps.get_model("db", "GitWebhookDelivery")
    GithubAppInstallation = apps.get_model("db", "GithubAppInstallation")
    GithubRepositorySync = apps.get_model("db", "GithubRepositorySync")
    GithubIssueSync = apps.get_model("db", "GithubIssueSync")
    GithubCommentSync = apps.get_model("db", "GithubCommentSync")
    GithubPullRequestLink = apps.get_model("db", "GithubPullRequestLink")
    GithubWebhookDelivery = apps.get_model("db", "GithubWebhookDelivery")

    provider_accounts_by_wi = {}

    for wi in WorkspaceIntegration.objects.filter(integration__provider="github"):
        config = wi.config or {}
        token = config.get("token") or ""
        if token:
            account, _ = GitProviderAccount.objects.get_or_create(
                workspace_id=wi.workspace_id,
                provider="github",
                host_url=GITHUB_HOST,
                auth_type="pat",
                external_account_id=f"pat:{wi.id}",
                defaults={
                    "external_account_login": config.get("github_user_login") or "",
                    "display_name": config.get("github_user_login") or "GitHub PAT",
                    "capabilities": {
                        "read_repositories": True,
                        "read_issues": True,
                        "write_comments": True,
                        "manage_webhooks": False,
                        "clone": False,
                    },
                    "credential_config": {
                        "auth_type": "pat",
                        "token": token,
                    },
                    "workspace_integration_id": wi.id,
                    "status": "connected",
                    "metadata": {
                        "source": "github_workspace_integration",
                        "verified_at": config.get("verified_at"),
                    },
                    "created_by_id": wi.created_by_id,
                    "updated_by_id": wi.updated_by_id,
                },
            )
            provider_accounts_by_wi[wi.id] = account

    for app in GithubAppInstallation.objects.select_related("workspace_integration"):
        wi = app.workspace_integration
        account, _ = GitProviderAccount.objects.get_or_create(
            workspace_id=wi.workspace_id,
            provider="github",
            host_url=GITHUB_HOST,
            auth_type="github_app",
            external_account_id=str(app.installation_id),
            defaults={
                "external_account_login": app.account_login,
                "display_name": app.account_login or f"GitHub App {app.installation_id}",
                "capabilities": {
                    "read_repositories": True,
                    "read_issues": True,
                    "write_comments": False,
                    "manage_webhooks": True,
                    "clone": False,
                },
                "credential_config": {
                    "auth_type": "github_app",
                    "installation_id": app.installation_id,
                },
                "workspace_integration_id": wi.id,
                "status": "degraded" if app.suspended_at else "connected",
                "verified_at": app.verified_at,
                "last_check_error": app.last_check_error,
                "metadata": {
                    "permissions": app.permissions,
                    "events": app.events,
                    "repository_selection": app.repository_selection,
                    "repository_count": app.repository_count,
                    "installed_at": app.installed_at.isoformat() if app.installed_at else None,
                    "suspended_at": app.suspended_at.isoformat() if app.suspended_at else None,
                },
                "created_by_id": app.created_by_id,
                "updated_by_id": app.updated_by_id,
            },
        )
        provider_accounts_by_wi.setdefault(wi.id, account)

    def account_for_sync(sync):
        account = provider_accounts_by_wi.get(sync.workspace_integration_id)
        if account is not None:
            return account
        wi = sync.workspace_integration
        account = GitProviderAccount.objects.create(
            workspace_id=sync.workspace_id,
            provider="github",
            host_url=GITHUB_HOST,
            auth_type="pat",
            external_account_id=f"legacy:{wi.id}",
            display_name="Legacy GitHub integration",
            capabilities={
                "read_repositories": True,
                "read_issues": True,
                "write_comments": True,
                "manage_webhooks": False,
                "clone": False,
            },
            credential_config={
                "auth_type": "pat",
                "token": (wi.config or {}).get("token") or "",
            },
            workspace_integration_id=wi.id,
            status="degraded",
            last_check_error="Backfilled from legacy GitHub sync without an active credential",
            created_by_id=sync.created_by_id,
            updated_by_id=sync.updated_by_id,
        )
        provider_accounts_by_wi[wi.id] = account
        return account

    binding_by_legacy_sync_id = {}
    repository_by_legacy_repo_id = {}

    for sync in GithubRepositorySync.objects.select_related("repository", "workspace_integration", "project", "actor"):
        legacy_repo = sync.repository
        namespace = (legacy_repo.owner or "").lower()
        name = (legacy_repo.name or "").lower()
        full_name = f"{namespace}/{name}".strip("/")
        web_url = legacy_repo.url or f"{GITHUB_HOST}/{full_name}"
        repo, _ = GitRepository.objects.get_or_create(
            provider="github",
            host_url=GITHUB_HOST,
            external_id=str(legacy_repo.repository_id),
            defaults={
                "namespace": namespace,
                "name": name,
                "full_name": full_name,
                "web_url": web_url,
                "clone_url_http": f"{web_url}.git" if web_url else "",
                "default_branch": "",
                "metadata": legacy_repo.config or {},
                "created_by_id": legacy_repo.created_by_id,
                "updated_by_id": legacy_repo.updated_by_id,
            },
        )
        repository_by_legacy_repo_id[legacy_repo.id] = repo
        account = account_for_sync(sync)
        binding, _ = GitRepositoryBinding.objects.get_or_create(
            project_id=sync.project_id,
            defaults={
                "workspace_id": sync.workspace_id,
                "repository_id": repo.id,
                "provider_account_id": account.id,
                "actor_id": sync.actor_id,
                "is_sync_enabled": sync.is_sync_enabled,
                "clone_auth_mode": "runner_managed",
                "last_synced_at": sync.last_synced_at,
                "last_sync_error": sync.last_sync_error,
                "metadata": {
                    "legacy_github_repository_sync_id": str(sync.id),
                    "legacy_label_id": str(sync.label_id) if sync.label_id else None,
                },
                "created_by_id": sync.created_by_id,
                "updated_by_id": sync.updated_by_id,
            },
        )
        binding_by_legacy_sync_id[sync.id] = binding

    issue_sync_by_legacy_id = {}
    for old in GithubIssueSync.objects.select_related("repository_sync", "issue"):
        binding = binding_by_legacy_sync_id.get(old.repository_sync_id)
        if binding is None:
            continue
        issue_sync, _ = GitIssueSync.objects.get_or_create(
            binding_id=binding.id,
            external_iid=str(old.repo_issue_id),
            defaults={
                "workspace_id": old.workspace_id,
                "project_id": old.project_id,
                "issue_id": old.issue_id,
                "provider": "github",
                "external_id": str(old.github_issue_id),
                "web_url": old.issue_url,
                "remote_state": "",
                "remote_created_at": old.gh_issue_created_at,
                "remote_updated_at": old.gh_issue_updated_at,
                "metadata": old.metadata or {},
                "created_by_id": old.created_by_id,
                "updated_by_id": old.updated_by_id,
            },
        )
        issue_sync_by_legacy_id[old.id] = issue_sync

    for old in GithubCommentSync.objects.select_related("issue_sync", "comment"):
        issue_sync = issue_sync_by_legacy_id.get(old.issue_sync_id)
        if issue_sync is None:
            continue
        GitCommentSync.objects.get_or_create(
            issue_sync_id=issue_sync.id,
            external_id=str(old.repo_comment_id),
            defaults={
                "workspace_id": old.workspace_id,
                "project_id": old.project_id,
                "comment_id": old.comment_id,
                "provider": "github",
                "metadata": {},
                "created_by_id": old.created_by_id,
                "updated_by_id": old.updated_by_id,
            },
        )

    for old in GithubPullRequestLink.objects.select_related("issue"):
        namespace = (old.repo_owner or "").lower()
        repo_name = (old.repo_name or "").lower()
        repo_external_id = ""
        repo = GitRepository.objects.filter(
            provider="github",
            host_url=GITHUB_HOST,
            namespace=namespace,
            name=repo_name,
        ).first()
        if repo is not None:
            repo_external_id = repo.external_id
        GitCodeReviewLink.objects.get_or_create(
            provider="github",
            host_url=GITHUB_HOST,
            namespace=namespace,
            repo_name=repo_name,
            repo_external_id=repo_external_id,
            external_iid=str(old.pr_number),
            defaults={
                "workspace_id": old.workspace_id,
                "project_id": old.project_id,
                "issue_id": old.issue_id,
                "external_id": "",
                "url": old.url,
                "title": old.title,
                "state": "closed" if old.state == "closed" else "open",
                "merged": old.merged,
                "draft": old.draft,
                "remote_updated_at": old.pr_updated_at,
                "metadata": {"legacy_github_pull_request_link_id": str(old.id)},
                "created_by_id": old.created_by_id,
                "updated_by_id": old.updated_by_id,
            },
        )

    for old in GithubWebhookDelivery.objects.all():
        GitWebhookDelivery.objects.get_or_create(
            provider="github",
            host_url=GITHUB_HOST,
            delivery_id=str(old.delivery_id),
            defaults={
                "event": old.event,
                "action": old.action,
                "payload": old.payload or {},
                "status": old.status,
                "processed_at": old.processed_at,
                "error": old.error,
                "metadata": {"legacy_installation_id": old.installation_id},
                "created_by_id": old.created_by_id,
                "updated_by_id": old.updated_by_id,
            },
        )


def noop_reverse(apps, schema_editor):
    # Legacy Github* tables remain in place, so reversing only drops generic
    # tables through the schema operations.
    return None


class Migration(migrations.Migration):

    dependencies = [
        ("db", "0151_github_pr_issue_link"),
        migrations.swappable_dependency(settings.AUTH_USER_MODEL),
    ]

    operations = [
        migrations.CreateModel(
            name="GitProviderAccount",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
                ("id", models.UUIDField(db_index=True, default=uuid.uuid4, editable=False, primary_key=True, serialize=False, unique=True)),
                ("provider", models.CharField(choices=[("github", "GitHub"), ("gitlab", "GitLab")], max_length=32)),
                ("host_url", models.URLField(max_length=500)),
                ("auth_type", models.CharField(choices=[("github_app", "GitHub App"), ("pat", "Personal Access Token"), ("oauth", "OAuth"), ("group_token", "Group Token"), ("project_token", "Project Token")], max_length=32)),
                ("external_account_id", models.CharField(blank=True, default="", max_length=255)),
                ("external_account_login", models.CharField(blank=True, default="", max_length=255)),
                ("display_name", models.CharField(blank=True, default="", max_length=255)),
                ("capabilities", models.JSONField(default=dict)),
                ("credential_config", models.JSONField(default=dict)),
                ("status", models.CharField(choices=[("connected", "Connected"), ("degraded", "Degraded"), ("revoked", "Revoked"), ("error", "Error")], default="connected", max_length=16)),
                ("verified_at", models.DateTimeField(blank=True, null=True)),
                ("last_check_error", models.TextField(blank=True, default="")),
                ("metadata", models.JSONField(default=dict)),
                ("created_by", models.ForeignKey(null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="%(class)s_created_by", to=settings.AUTH_USER_MODEL, verbose_name="Created By")),
                ("updated_by", models.ForeignKey(null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="%(class)s_updated_by", to=settings.AUTH_USER_MODEL, verbose_name="Last Modified By")),
                ("workspace", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="git_provider_accounts", to="db.workspace")),
                ("workspace_integration", models.ForeignKey(blank=True, null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="git_provider_accounts", to="db.workspaceintegration")),
            ],
            options={"db_table": "git_provider_accounts", "ordering": ("-created_at",), "verbose_name": "Git Provider Account", "verbose_name_plural": "Git Provider Accounts"},
        ),
        migrations.CreateModel(
            name="GitRepository",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
                ("id", models.UUIDField(db_index=True, default=uuid.uuid4, editable=False, primary_key=True, serialize=False, unique=True)),
                ("provider", models.CharField(choices=[("github", "GitHub"), ("gitlab", "GitLab")], max_length=32)),
                ("host_url", models.URLField(max_length=500)),
                ("external_id", models.CharField(blank=True, default="", max_length=255)),
                ("namespace", models.CharField(max_length=500)),
                ("name", models.CharField(max_length=500)),
                ("full_name", models.CharField(max_length=1000)),
                ("web_url", models.URLField(blank=True, default="", max_length=1000)),
                ("clone_url_http", models.CharField(blank=True, default="", max_length=1000)),
                ("clone_url_ssh", models.CharField(blank=True, default="", max_length=1000)),
                ("default_branch", models.CharField(blank=True, default="", max_length=255)),
                ("is_private", models.BooleanField(default=False)),
                ("metadata", models.JSONField(default=dict)),
                ("created_by", models.ForeignKey(null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="%(class)s_created_by", to=settings.AUTH_USER_MODEL, verbose_name="Created By")),
                ("updated_by", models.ForeignKey(null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="%(class)s_updated_by", to=settings.AUTH_USER_MODEL, verbose_name="Last Modified By")),
            ],
            options={"db_table": "git_repositories", "ordering": ("-created_at",), "verbose_name": "Git Repository", "verbose_name_plural": "Git Repositories"},
        ),
        migrations.CreateModel(
            name="GitRepositoryBinding",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
                ("id", models.UUIDField(db_index=True, default=uuid.uuid4, editable=False, primary_key=True, serialize=False, unique=True)),
                ("is_sync_enabled", models.BooleanField(default=False)),
                ("clone_auth_mode", models.CharField(choices=[("public", "Public"), ("runner_managed", "Runner Managed"), ("managed_ephemeral", "Managed Ephemeral")], default="runner_managed", max_length=32)),
                ("last_synced_at", models.DateTimeField(blank=True, null=True)),
                ("last_sync_error", models.TextField(blank=True, default="")),
                ("metadata", models.JSONField(default=dict)),
                ("actor", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="git_repository_bindings", to=settings.AUTH_USER_MODEL)),
                ("created_by", models.ForeignKey(null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="%(class)s_created_by", to=settings.AUTH_USER_MODEL, verbose_name="Created By")),
                ("project", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="project_%(class)s", to="db.project")),
                ("provider_account", models.ForeignKey(on_delete=django.db.models.deletion.PROTECT, related_name="repository_bindings", to="db.gitprovideraccount")),
                ("repository", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="bindings", to="db.gitrepository")),
                ("updated_by", models.ForeignKey(null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="%(class)s_updated_by", to=settings.AUTH_USER_MODEL, verbose_name="Last Modified By")),
                ("workspace", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="workspace_%(class)s", to="db.workspace")),
            ],
            options={"db_table": "git_repository_bindings", "ordering": ("-created_at",), "verbose_name": "Git Repository Binding", "verbose_name_plural": "Git Repository Bindings"},
        ),
        migrations.CreateModel(
            name="GitIssueSync",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
                ("id", models.UUIDField(db_index=True, default=uuid.uuid4, editable=False, primary_key=True, serialize=False, unique=True)),
                ("provider", models.CharField(max_length=32)),
                ("external_id", models.CharField(blank=True, default="", max_length=255)),
                ("external_iid", models.CharField(max_length=255)),
                ("web_url", models.URLField(blank=True, default="", max_length=1000)),
                ("remote_state", models.CharField(blank=True, default="", max_length=64)),
                ("remote_created_at", models.DateTimeField(blank=True, null=True)),
                ("remote_updated_at", models.DateTimeField(blank=True, null=True)),
                ("metadata", models.JSONField(default=dict)),
                ("binding", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="issue_syncs", to="db.gitrepositorybinding")),
                ("created_by", models.ForeignKey(null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="%(class)s_created_by", to=settings.AUTH_USER_MODEL, verbose_name="Created By")),
                ("issue", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="git_syncs", to="db.issue")),
                ("project", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="project_%(class)s", to="db.project")),
                ("updated_by", models.ForeignKey(null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="%(class)s_updated_by", to=settings.AUTH_USER_MODEL, verbose_name="Last Modified By")),
                ("workspace", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="workspace_%(class)s", to="db.workspace")),
            ],
            options={"db_table": "git_issue_syncs", "ordering": ("-created_at",), "verbose_name": "Git Issue Sync", "verbose_name_plural": "Git Issue Syncs"},
        ),
        migrations.CreateModel(
            name="GitCommentSync",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
                ("id", models.UUIDField(db_index=True, default=uuid.uuid4, editable=False, primary_key=True, serialize=False, unique=True)),
                ("provider", models.CharField(max_length=32)),
                ("external_id", models.CharField(max_length=255)),
                ("remote_created_at", models.DateTimeField(blank=True, null=True)),
                ("remote_updated_at", models.DateTimeField(blank=True, null=True)),
                ("metadata", models.JSONField(default=dict)),
                ("comment", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="git_comment_syncs", to="db.issuecomment")),
                ("created_by", models.ForeignKey(null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="%(class)s_created_by", to=settings.AUTH_USER_MODEL, verbose_name="Created By")),
                ("issue_sync", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="comment_syncs", to="db.gitissuesync")),
                ("project", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="project_%(class)s", to="db.project")),
                ("updated_by", models.ForeignKey(null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="%(class)s_updated_by", to=settings.AUTH_USER_MODEL, verbose_name="Last Modified By")),
                ("workspace", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="workspace_%(class)s", to="db.workspace")),
            ],
            options={"db_table": "git_comment_syncs", "ordering": ("-created_at",), "verbose_name": "Git Comment Sync", "verbose_name_plural": "Git Comment Syncs"},
        ),
        migrations.CreateModel(
            name="GitCodeReviewLink",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
                ("id", models.UUIDField(db_index=True, default=uuid.uuid4, editable=False, primary_key=True, serialize=False, unique=True)),
                ("provider", models.CharField(max_length=32)),
                ("host_url", models.URLField(max_length=500)),
                ("namespace", models.CharField(max_length=500)),
                ("repo_name", models.CharField(max_length=500)),
                ("repo_external_id", models.CharField(blank=True, default="", max_length=255)),
                ("external_id", models.CharField(blank=True, default="", max_length=255)),
                ("external_iid", models.CharField(max_length=255)),
                ("url", models.URLField(max_length=1000)),
                ("title", models.CharField(blank=True, default="", max_length=500)),
                ("state", models.CharField(choices=[("open", "Open"), ("closed", "Closed"), ("merged", "Merged")], default="open", max_length=16)),
                ("merged", models.BooleanField(default=False)),
                ("draft", models.BooleanField(default=False)),
                ("remote_updated_at", models.DateTimeField(blank=True, null=True)),
                ("metadata", models.JSONField(default=dict)),
                ("created_by", models.ForeignKey(null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="%(class)s_created_by", to=settings.AUTH_USER_MODEL, verbose_name="Created By")),
                ("issue", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="git_code_reviews", to="db.issue")),
                ("project", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="project_%(class)s", to="db.project")),
                ("updated_by", models.ForeignKey(null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="%(class)s_updated_by", to=settings.AUTH_USER_MODEL, verbose_name="Last Modified By")),
                ("workspace", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="workspace_%(class)s", to="db.workspace")),
            ],
            options={"db_table": "git_code_review_links", "ordering": ("-created_at",), "verbose_name": "Git Code Review Link", "verbose_name_plural": "Git Code Review Links"},
        ),
        migrations.CreateModel(
            name="GitWebhookDelivery",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
                ("id", models.UUIDField(db_index=True, default=uuid.uuid4, editable=False, primary_key=True, serialize=False, unique=True)),
                ("provider", models.CharField(max_length=32)),
                ("host_url", models.URLField(blank=True, default="", max_length=500)),
                ("delivery_id", models.CharField(db_index=True, max_length=255)),
                ("event", models.CharField(max_length=100)),
                ("action", models.CharField(blank=True, default="", max_length=100)),
                ("raw_headers", models.JSONField(default=dict)),
                ("payload", models.JSONField(default=dict)),
                ("status", models.CharField(choices=[("received", "Received"), ("processed", "Processed"), ("failed", "Failed"), ("skipped", "Skipped")], default="received", max_length=16)),
                ("received_at", models.DateTimeField(auto_now_add=True)),
                ("processed_at", models.DateTimeField(blank=True, null=True)),
                ("error", models.TextField(blank=True, default="")),
                ("metadata", models.JSONField(default=dict)),
                ("created_by", models.ForeignKey(null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="%(class)s_created_by", to=settings.AUTH_USER_MODEL, verbose_name="Created By")),
                ("repository", models.ForeignKey(blank=True, null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="webhook_deliveries", to="db.gitrepository")),
                ("updated_by", models.ForeignKey(null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="%(class)s_updated_by", to=settings.AUTH_USER_MODEL, verbose_name="Last Modified By")),
            ],
            options={"db_table": "git_webhook_deliveries", "ordering": ("-received_at",), "verbose_name": "Git Webhook Delivery", "verbose_name_plural": "Git Webhook Deliveries"},
        ),
        migrations.CreateModel(
            name="GitWebhookRegistration",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
                ("id", models.UUIDField(db_index=True, default=uuid.uuid4, editable=False, primary_key=True, serialize=False, unique=True)),
                ("provider_hook_id", models.CharField(blank=True, default="", max_length=255)),
                ("events", models.JSONField(default=list)),
                ("secret_ref", models.CharField(blank=True, default="", max_length=255)),
                ("last_verified_at", models.DateTimeField(blank=True, null=True)),
                ("last_check_error", models.TextField(blank=True, default="")),
                ("metadata", models.JSONField(default=dict)),
                ("created_by", models.ForeignKey(null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="%(class)s_created_by", to=settings.AUTH_USER_MODEL, verbose_name="Created By")),
                ("provider_account", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="webhook_registrations", to="db.gitprovideraccount")),
                ("repository", models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, related_name="webhook_registrations", to="db.gitrepository")),
                ("updated_by", models.ForeignKey(null=True, on_delete=django.db.models.deletion.SET_NULL, related_name="%(class)s_updated_by", to=settings.AUTH_USER_MODEL, verbose_name="Last Modified By")),
            ],
            options={"db_table": "git_webhook_registrations", "ordering": ("-created_at",), "verbose_name": "Git Webhook Registration", "verbose_name_plural": "Git Webhook Registrations"},
        ),
        migrations.AddIndex(model_name="gitprovideraccount", index=models.Index(fields=["workspace", "provider", "host_url"], name="git_pa_ws_provider_host_idx")),
        migrations.AddIndex(model_name="gitprovideraccount", index=models.Index(fields=["provider", "host_url", "external_account_id"], name="git_pa_provider_ext_idx")),
        migrations.AddConstraint(model_name="gitrepository", constraint=models.UniqueConstraint(condition=models.Q(("deleted_at__isnull", True), models.Q(("external_id", ""), _negated=True)), fields=("provider", "host_url", "external_id"), name="git_repo_uniq_ext_active")),
        migrations.AddConstraint(model_name="gitrepository", constraint=models.UniqueConstraint(condition=models.Q(("deleted_at__isnull", True)), fields=("provider", "host_url", "full_name"), name="git_repo_uniq_full_active")),
        migrations.AddIndex(model_name="gitrepository", index=models.Index(fields=["provider", "host_url", "full_name"], name="git_repo_provider_full_idx")),
        migrations.AddConstraint(model_name="gitrepositorybinding", constraint=models.UniqueConstraint(condition=models.Q(("deleted_at__isnull", True)), fields=("project",), name="git_bind_uniq_project_active")),
        migrations.AddIndex(model_name="gitrepositorybinding", index=models.Index(fields=["workspace", "project"], name="git_bind_ws_project_idx")),
        migrations.AddIndex(model_name="gitrepositorybinding", index=models.Index(fields=["repository", "provider_account"], name="git_bind_repo_acct_idx")),
        migrations.AddConstraint(model_name="gitissuesync", constraint=models.UniqueConstraint(condition=models.Q(("deleted_at__isnull", True)), fields=("binding", "external_iid"), name="git_issue_uniq_iid_active")),
        migrations.AddConstraint(model_name="gitissuesync", constraint=models.UniqueConstraint(condition=models.Q(("deleted_at__isnull", True)), fields=("binding", "issue"), name="git_issue_uniq_issue_active")),
        migrations.AddIndex(model_name="gitissuesync", index=models.Index(fields=["issue"], name="git_issue_sync_issue_idx")),
        migrations.AddIndex(model_name="gitissuesync", index=models.Index(fields=["provider", "external_iid"], name="git_issue_provider_iid_idx")),
        migrations.AddConstraint(model_name="gitcommentsync", constraint=models.UniqueConstraint(condition=models.Q(("deleted_at__isnull", True)), fields=("issue_sync", "external_id"), name="git_comment_uniq_ext_active")),
        migrations.AddConstraint(model_name="gitcommentsync", constraint=models.UniqueConstraint(condition=models.Q(("deleted_at__isnull", True)), fields=("issue_sync", "comment"), name="git_comment_uniq_comment_act")),
        migrations.AddIndex(model_name="gitcommentsync", index=models.Index(fields=["comment"], name="git_comment_comment_idx")),
        migrations.AddConstraint(model_name="gitcodereviewlink", constraint=models.UniqueConstraint(condition=models.Q(("deleted_at__isnull", True), models.Q(("repo_external_id", ""), _negated=True)), fields=("provider", "host_url", "repo_external_id", "external_iid"), name="git_cr_uniq_repo_iid_active")),
        migrations.AddConstraint(model_name="gitcodereviewlink", constraint=models.UniqueConstraint(condition=models.Q(("deleted_at__isnull", True), ("repo_external_id", "")), fields=("provider", "host_url", "namespace", "repo_name", "external_iid"), name="git_cr_uniq_path_iid_active")),
        migrations.AddIndex(model_name="gitcodereviewlink", index=models.Index(fields=["issue"], name="git_cr_issue_idx")),
        migrations.AddIndex(model_name="gitcodereviewlink", index=models.Index(fields=["provider", "host_url", "namespace", "repo_name"], name="git_cr_provider_repo_idx")),
        migrations.AddConstraint(model_name="gitwebhookdelivery", constraint=models.UniqueConstraint(condition=models.Q(("deleted_at__isnull", True)), fields=("provider", "host_url", "delivery_id"), name="git_wh_delivery_uniq_active")),
        migrations.AddConstraint(model_name="gitwebhookregistration", constraint=models.UniqueConstraint(condition=models.Q(("deleted_at__isnull", True)), fields=("repository", "provider_account"), name="git_wh_reg_uniq_repo_acct")),
        migrations.RunPython(backfill_github_to_generic, noop_reverse),
    ]
