# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

from django.db import transaction
from django.shortcuts import get_object_or_404
from django.utils import timezone

from pi_dash.db.models import (
    GitProviderAccount,
    GitRepository,
    GitRepositoryBinding,
    GithubRepositorySync,
    Project,
    Workspace,
)
from pi_dash.integrations.git.adapters.base import (
    GitProviderAuthError,
    GitProviderNotFoundError,
    GitProviderPermissionError,
)
from pi_dash.integrations.git.dtos import RemoteRepository
from pi_dash.integrations.git.registry import get_adapter, parse_repository_url
from pi_dash.license.utils.encryption import encrypt_data


class GitIntegrationError(Exception):
    status_code = 400


class UnsupportedRepositoryURL(GitIntegrationError):
    status_code = 400


class ProviderAccountRequired(GitIntegrationError):
    status_code = 409


class ProviderAccountAmbiguous(GitIntegrationError):
    status_code = 409


class ProviderAccountNotFound(GitIntegrationError):
    status_code = 404


def normalize_host_url(host_url: str) -> str:
    host_url = (host_url or "").strip().rstrip("/")
    if host_url and not host_url.startswith(("http://", "https://")):
        host_url = f"https://{host_url}"
    return host_url.rstrip("/")


def account_credential(account: GitProviderAccount) -> dict:
    config = dict(account.credential_config or {})
    config.setdefault("auth_type", account.auth_type)
    config.setdefault("host_url", account.host_url)
    return config


def create_provider_account(
    *,
    workspace: Workspace,
    actor,
    provider: str,
    host_url: str,
    auth_type: str,
    token: str,
) -> GitProviderAccount:
    adapter = get_adapter(provider)
    normalized_host_url = normalize_host_url(host_url)
    credential = {
        "auth_type": auth_type,
        "host_url": normalized_host_url,
        "token": token,
    }
    identity = adapter.verify_provider_account(credential)
    capabilities = adapter.credential_capabilities(credential).as_dict()
    encrypted_token = encrypt_data(token)

    external_id = str(identity.get("id") or identity.get("username") or identity.get("login") or "")
    login = identity.get("login") or identity.get("username") or identity.get("name") or ""
    display_name = login or f"{adapter.display_name} account"
    account = GitProviderAccount.objects.create(
        workspace=workspace,
        provider=provider,
        host_url=normalized_host_url,
        auth_type=auth_type,
        external_account_id=external_id,
        external_account_login=login,
        display_name=display_name,
        capabilities=capabilities,
        credential_config={
            "auth_type": auth_type,
            "host_url": normalized_host_url,
            "token": encrypted_token,
        },
        status=GitProviderAccount.Status.CONNECTED,
        verified_at=timezone.now(),
        metadata={"identity": identity},
        created_by_id=getattr(actor, "id", None),
        updated_by_id=getattr(actor, "id", None),
    )
    return account


def select_provider_account(
    *,
    workspace: Workspace,
    provider: str,
    host_url: str,
    provider_account_id=None,
) -> GitProviderAccount:
    normalized_host_url = normalize_host_url(host_url)
    queryset = GitProviderAccount.objects.filter(
        workspace=workspace,
        provider=provider,
        host_url=normalized_host_url,
        status__in=[GitProviderAccount.Status.CONNECTED, GitProviderAccount.Status.DEGRADED],
    )
    if provider_account_id:
        account = queryset.filter(id=provider_account_id).first()
        if account is None:
            raise ProviderAccountNotFound("Provider account not found for this repository host")
        return account
    count = queryset.count()
    if count == 0:
        raise ProviderAccountRequired("Connect a provider account before binding this repository")
    if count > 1:
        raise ProviderAccountAmbiguous("Multiple provider accounts can access this host; choose one")
    return queryset.first()


def upsert_repository(remote: RemoteRepository, *, host_url: str) -> GitRepository:
    lookup = {
        "provider": remote.provider,
        "host_url": host_url.rstrip("/"),
    }
    if remote.external_id:
        lookup["external_id"] = remote.external_id
    else:
        lookup["full_name"] = remote.full_name
    defaults = {
        "external_id": remote.external_id,
        "namespace": remote.namespace,
        "name": remote.name,
        "full_name": remote.full_name,
        "web_url": remote.web_url,
        "clone_url_http": remote.clone_url_http,
        "clone_url_ssh": remote.clone_url_ssh,
        "default_branch": remote.default_branch,
        "is_private": remote.is_private,
        "metadata": remote.metadata,
    }
    repo, _ = GitRepository.objects.update_or_create(defaults=defaults, **lookup)
    return repo


def canonical_clone_url(remote: RemoteRepository, raw_url: str) -> str:
    return remote.clone_url_http or raw_url or remote.web_url


def serialize_repository(repo: GitRepository) -> dict:
    return {
        "id": repo.external_id,
        "provider": repo.provider,
        "host_url": repo.host_url,
        "namespace": repo.namespace,
        "name": repo.name,
        "full_name": repo.full_name,
        "web_url": repo.web_url,
        "clone_url_http": repo.clone_url_http,
        "clone_url_ssh": repo.clone_url_ssh,
        "default_branch": repo.default_branch,
        "private": repo.is_private,
    }


def serialize_remote_repository(repo: RemoteRepository, *, host_url: str) -> dict:
    return {
        "id": repo.external_id,
        "provider": repo.provider,
        "host_url": host_url,
        "namespace": repo.namespace,
        "name": repo.name,
        "full_name": repo.full_name,
        "web_url": repo.web_url,
        "clone_url_http": repo.clone_url_http,
        "clone_url_ssh": repo.clone_url_ssh,
        "default_branch": repo.default_branch,
        "private": repo.is_private,
    }


def serialize_provider_account(account: GitProviderAccount) -> dict:
    return {
        "id": str(account.id),
        "provider": account.provider,
        "host_url": account.host_url,
        "auth_type": account.auth_type,
        "external_account_id": account.external_account_id,
        "external_account_login": account.external_account_login,
        "display_name": account.display_name,
        "capabilities": account.capabilities,
        "status": account.status,
        "verified_at": account.verified_at.isoformat() if account.verified_at else None,
        "last_check_error": account.last_check_error,
    }


def serialize_binding(binding: GitRepositoryBinding) -> dict:
    return {
        "bound": True,
        "id": str(binding.id),
        "provider": binding.repository.provider,
        "provider_account_id": str(binding.provider_account_id),
        "host_url": binding.repository.host_url,
        "repository": serialize_repository(binding.repository),
        "is_sync_enabled": binding.is_sync_enabled,
        "clone_auth_mode": binding.clone_auth_mode,
        "last_synced_at": binding.last_synced_at.isoformat() if binding.last_synced_at else None,
        "last_sync_error": binding.last_sync_error,
        "degraded": binding.provider_account.status != GitProviderAccount.Status.CONNECTED,
        "degraded_reason": binding.provider_account.last_check_error,
    }


def bind_repository(
    *,
    workspace_slug: str,
    project_id,
    actor,
    raw_url: str,
    provider_account_id=None,
) -> tuple[GitRepositoryBinding, str]:
    parsed = parse_repository_url(raw_url)
    if parsed is None:
        raise UnsupportedRepositoryURL("A supported GitHub or GitLab repository URL is required")

    workspace = get_object_or_404(Workspace, slug=workspace_slug)
    project = get_object_or_404(Project, pk=project_id, workspace=workspace)
    account = select_provider_account(
        workspace=workspace,
        provider=parsed.provider,
        host_url=parsed.host_url,
        provider_account_id=provider_account_id,
    )
    adapter = get_adapter(parsed.provider)
    remote = adapter.get_repository(account_credential(account), parsed)
    repo = upsert_repository(remote, host_url=parsed.host_url)
    clone_url = canonical_clone_url(remote, parsed.clone_url)
    clone_auth_mode = (
        GitRepositoryBinding.CloneAuthMode.RUNNER_MANAGED
        if remote.is_private
        else GitRepositoryBinding.CloneAuthMode.PUBLIC
    )

    with transaction.atomic():
        existing = GitRepositoryBinding.objects.filter(project=project).first()
        if existing is not None:
            existing.delete(soft=False)
        GithubRepositorySync.objects.filter(project=project).delete(soft=False)
        binding = GitRepositoryBinding.objects.create(
            project=project,
            workspace=workspace,
            repository=repo,
            provider_account=account,
            actor=actor,
            is_sync_enabled=False,
            clone_auth_mode=clone_auth_mode,
            metadata={"raw_url": raw_url},
            created_by_id=getattr(actor, "id", None),
            updated_by_id=getattr(actor, "id", None),
        )
        update_fields = []
        if project.repo_url != clone_url:
            project.repo_url = clone_url
            update_fields.append("repo_url")
        if remote.default_branch and not project.base_branch:
            project.base_branch = remote.default_branch
            update_fields.append("base_branch")
        if update_fields:
            project.save(update_fields=update_fields)
    return binding, clone_url


def get_binding(*, workspace_slug: str, project_id) -> GitRepositoryBinding | None:
    return (
        GitRepositoryBinding.objects.filter(project_id=project_id, workspace__slug=workspace_slug)
        .select_related("repository", "provider_account")
        .first()
    )


def set_binding_sync_enabled(*, workspace_slug: str, project_id, enabled: bool) -> GitRepositoryBinding:
    binding = get_binding(workspace_slug=workspace_slug, project_id=project_id)
    if binding is None:
        raise ProviderAccountNotFound("Repository is not bound")
    binding.is_sync_enabled = enabled
    binding.save(update_fields=["is_sync_enabled", "updated_at"])
    if binding.repository.provider == "github":
        GithubRepositorySync.objects.filter(project_id=project_id, workspace__slug=workspace_slug).update(
            is_sync_enabled=enabled,
        )
    return binding


def unbind_repository(*, workspace_slug: str, project_id) -> None:
    binding = get_binding(workspace_slug=workspace_slug, project_id=project_id)
    if binding is not None:
        binding.delete(soft=False)
    GithubRepositorySync.objects.filter(project_id=project_id, workspace__slug=workspace_slug).delete(soft=False)


def list_account_repositories(account: GitProviderAccount, *, page: int = 1) -> dict:
    adapter = get_adapter(account.provider)
    repo_page = adapter.list_repositories(account_credential(account), page=page)
    return {
        "repos": [serialize_remote_repository(repo, host_url=account.host_url) for repo in repo_page.repositories],
        "page": repo_page.page,
        "has_next_page": repo_page.has_next_page,
    }
