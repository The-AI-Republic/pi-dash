# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# PDASHOSS01-356 (github_* sunset 3/6): the 0173 backfill moves GitHub App
# installations and install sessions off the legacy tables onto the
# generic ones. These tests pin the idempotent mapping: installations
# land on GitProviderAccount matched by external_account_id, sessions on
# GitProviderInstallSession matched by state.

import importlib

import pytest
from django.apps import apps as django_apps
from django.utils import timezone

from pi_dash.app.views.integration.github import _get_or_create_workspace_integration
from pi_dash.db.models import (
    GitProviderAccount,
    GitProviderInstallSession,
    GithubAppInstallation,
    GithubAppInstallSession,
)


pytestmark = [pytest.mark.unit, pytest.mark.django_db]

migration_module = importlib.import_module("pi_dash.db.migrations.0173_git_provider_app_columns")


@pytest.fixture
def workspace_integration(workspace, create_user):
    return _get_or_create_workspace_integration(workspace, create_user)


@pytest.fixture
def legacy_installation(workspace_integration, create_user):
    now = timezone.now()
    return GithubAppInstallation.objects.create(
        workspace_integration=workspace_integration,
        installation_id=98765,
        account_login="acme-corp",
        account_type="Organization",
        repository_selection="selected",
        repository_count=3,
        permissions={"metadata": "read"},
        events=["installation", "installation_repositories"],
        installed_at=now,
        suspended_at=None,
        verified_at=now,
        last_checked_at=now,
        last_check_error="",
        created_by=create_user,
    )


def test_backfill_installations_creates_generic_account(legacy_installation, workspace):
    migration_module.backfill_app_installations(django_apps, None)

    account = GitProviderAccount.objects.get(
        workspace=workspace,
        provider="github",
        host_url="https://github.com",
        auth_type="github_app",
        external_account_id="98765",
    )
    assert account.external_account_login == "acme-corp"
    assert account.display_name == "acme-corp"
    assert account.status == "connected"
    assert account.suspended_at is None
    assert account.last_checked_at is not None
    assert account.verified_at is not None
    assert account.credential_config["installation_id"] == 98765
    assert account.metadata["account_type"] == "Organization"
    assert account.metadata["repository_count"] == 3
    assert account.metadata["permissions"] == {"metadata": "read"}
    assert account.workspace_integration_id == legacy_installation.workspace_integration_id


def test_backfill_installations_is_idempotent(legacy_installation):
    migration_module.backfill_app_installations(django_apps, None)
    migration_module.backfill_app_installations(django_apps, None)

    assert GitProviderAccount.objects.filter(auth_type="github_app").count() == 1


def test_backfill_installations_carries_suspended_state(legacy_installation):
    legacy_installation.suspended_at = legacy_installation.verified_at
    legacy_installation.save(update_fields=["suspended_at", "updated_at"])

    migration_module.backfill_app_installations(django_apps, None)

    account = GitProviderAccount.objects.get(external_account_id="98765")
    assert account.suspended_at is not None
    assert account.status == "degraded"
    assert account.metadata["suspended_at"] is not None


def test_backfill_sessions_copies_active_session(workspace, workspace_integration, create_user):
    now = timezone.now()
    GithubAppInstallSession.objects.create(
        state="test-state-1",
        workspace=workspace,
        actor=create_user,
        installation_id=98765,
        account_login="acme-corp",
        status=GithubAppInstallSession.Status.STARTED,
        expires_at=now,
        created_by=create_user,
    )

    migration_module.backfill_install_sessions(django_apps, None)
    migration_module.backfill_install_sessions(django_apps, None)

    session = GitProviderInstallSession.objects.get(state="test-state-1")
    assert session.provider == "github"
    assert session.workspace_id == workspace.id
    assert session.actor_id == create_user.id
    assert session.external_account_id == "98765"
    assert session.account_login == "acme-corp"
    assert session.status == "started"
    assert GitProviderInstallSession.objects.filter(state="test-state-1").count() == 1


def test_backfill_installations_syncs_health_columns_on_existing_account(legacy_installation):
    migration_module.backfill_app_installations(django_apps, None)

    # Django refreshes keep writing the legacy row after the first
    # backfill (0152-time values go stale); the rerun must pick up the
    # authoritative health columns on the existing account.
    legacy_installation.last_check_error = "GitHub App API unreachable"
    legacy_installation.save(update_fields=["last_check_error", "updated_at"])
    migration_module.backfill_app_installations(django_apps, None)

    account = GitProviderAccount.objects.get(external_account_id="98765")
    assert account.last_check_error == "GitHub App API unreachable"
    assert account.verified_at == legacy_installation.verified_at
    assert GitProviderAccount.objects.filter(auth_type="github_app").count() == 1
