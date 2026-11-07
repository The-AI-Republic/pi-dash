# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# PDASHOSS01-356 (github_* sunset 3/6): give git_provider_accounts typed
# operational columns for GitHub App installations and a generic
# install-session table. The legacy github_app_installations /
# github_app_install_sessions tables keep their rows; this migration
# backfills the generic tables from them so the Rust install flow can
# read/write only the generic tables. The backfill is idempotent:
# installations are matched on (workspace, provider, host_url,
# auth_type, external_account_id) and sessions on state.

import uuid

import django.db.models.deletion
from django.conf import settings
from django.db import migrations, models


GITHUB_HOST = "https://github.com"


def backfill_app_installations(apps, schema_editor):
    GitProviderAccount = apps.get_model("db", "GitProviderAccount")
    GithubAppInstallation = apps.get_model("db", "GithubAppInstallation")

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
                "suspended_at": app.suspended_at,
                "last_checked_at": app.last_checked_at,
                "last_check_error": app.last_check_error,
                "metadata": {
                    "permissions": app.permissions,
                    "events": app.events,
                    "repository_selection": app.repository_selection,
                    "repository_count": app.repository_count,
                    "account_type": app.account_type,
                    "installed_at": app.installed_at.isoformat() if app.installed_at else None,
                    "suspended_at": app.suspended_at.isoformat() if app.suspended_at else None,
                },
                "created_by_id": app.created_by_id,
                "updated_by_id": app.updated_by_id,
            },
        )
        # Existing accounts (created by 0152 or by the Rust companion
        # upsert): the legacy row is authoritative until the cutover,
        # so sync the operational columns and fill account_type, which
        # 0152 did not store. github_app status is only ever
        # connected/degraded, derived from suspended_at.
        account.suspended_at = app.suspended_at
        account.last_checked_at = app.last_checked_at
        account.verified_at = app.verified_at
        account.last_check_error = app.last_check_error
        account.status = "degraded" if app.suspended_at else "connected"
        metadata = account.metadata or {}
        metadata.update(
            {
                "permissions": app.permissions,
                "events": app.events,
                "repository_selection": app.repository_selection,
                "repository_count": app.repository_count,
                "account_type": app.account_type,
                "installed_at": app.installed_at.isoformat() if app.installed_at else None,
                "suspended_at": app.suspended_at.isoformat() if app.suspended_at else None,
            }
        )
        account.metadata = metadata
        account.save(
            update_fields=[
                "suspended_at",
                "last_checked_at",
                "verified_at",
                "last_check_error",
                "status",
                "metadata",
            ]
        )


def backfill_install_sessions(apps, schema_editor):
    GitProviderInstallSession = apps.get_model("db", "GitProviderInstallSession")
    GithubAppInstallSession = apps.get_model("db", "GithubAppInstallSession")

    for session in GithubAppInstallSession.objects.all():
        GitProviderInstallSession.objects.get_or_create(
            state=session.state,
            defaults={
                "provider": "github",
                "workspace_id": session.workspace_id,
                "actor_id": session.actor_id,
                "external_account_id": (str(session.installation_id) if session.installation_id is not None else None),
                "account_login": session.account_login,
                "status": session.status,
                "expires_at": session.expires_at,
                "completed_at": session.completed_at,
                "error": session.error,
                "created_by_id": session.created_by_id,
                "updated_by_id": session.updated_by_id,
            },
        )


class Migration(migrations.Migration):
    dependencies = [
        ("db", "0172_clidevicecode_engine_scope"),
        ("db", "0152_git_generalization"),
    ]

    operations = [
        migrations.AddField(
            model_name="gitprovideraccount",
            name="suspended_at",
            field=models.DateTimeField(blank=True, null=True),
        ),
        migrations.AddField(
            model_name="gitprovideraccount",
            name="last_checked_at",
            field=models.DateTimeField(blank=True, null=True),
        ),
        migrations.CreateModel(
            name="GitProviderInstallSession",
            fields=[
                ("created_at", models.DateTimeField(auto_now_add=True, verbose_name="Created At")),
                ("updated_at", models.DateTimeField(auto_now=True, verbose_name="Last Modified At")),
                ("deleted_at", models.DateTimeField(blank=True, null=True, verbose_name="Deleted At")),
                (
                    "id",
                    models.UUIDField(
                        db_index=True,
                        default=uuid.uuid4,
                        editable=False,
                        primary_key=True,
                        serialize=False,
                        unique=True,
                    ),
                ),
                ("state", models.CharField(db_index=True, max_length=128, unique=True)),
                ("provider", models.CharField(max_length=32)),
                ("external_account_id", models.CharField(blank=True, max_length=255, null=True)),
                ("account_login", models.CharField(blank=True, default="", max_length=255)),
                (
                    "status",
                    models.CharField(
                        choices=[
                            ("started", "Started"),
                            ("completed", "Completed"),
                            ("expired", "Expired"),
                            ("failed", "Failed"),
                        ],
                        db_index=True,
                        default="started",
                        max_length=16,
                    ),
                ),
                ("expires_at", models.DateTimeField(db_index=True)),
                ("completed_at", models.DateTimeField(blank=True, null=True)),
                ("error", models.TextField(blank=True, default="")),
                (
                    "created_by",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="%(class)s_created_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Created By",
                    ),
                ),
                (
                    "updated_by",
                    models.ForeignKey(
                        null=True,
                        on_delete=django.db.models.deletion.SET_NULL,
                        related_name="%(class)s_updated_by",
                        to=settings.AUTH_USER_MODEL,
                        verbose_name="Last Modified By",
                    ),
                ),
                (
                    "actor",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="git_provider_install_sessions",
                        to="db.user",
                    ),
                ),
                (
                    "workspace",
                    models.ForeignKey(
                        on_delete=django.db.models.deletion.CASCADE,
                        related_name="git_provider_install_sessions",
                        to="db.workspace",
                    ),
                ),
            ],
            options={
                "db_table": "git_provider_install_sessions",
                "ordering": ("-created_at",),
                "verbose_name": "Git Provider Install Session",
                "verbose_name_plural": "Git Provider Install Sessions",
            },
        ),
        migrations.RunPython(backfill_app_installations, migrations.RunPython.noop),
        migrations.RunPython(backfill_install_sessions, migrations.RunPython.noop),
    ]
