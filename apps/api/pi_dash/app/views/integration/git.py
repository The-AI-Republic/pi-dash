# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

from django.conf import settings
from django.shortcuts import get_object_or_404
from rest_framework import status
from rest_framework.response import Response

from pi_dash.app.permissions import ROLE, allow_permission
from pi_dash.app.views.base import BaseAPIView
from pi_dash.db.models import GitProviderAccount, Workspace
from pi_dash.integrations.git.adapters.base import (
    GitProviderAuthError,
    GitProviderNotFoundError,
    GitProviderPermissionError,
)
from pi_dash.integrations.git.registry import provider_payload
from pi_dash.integrations.git.services import (
    GitIntegrationError,
    bind_repository,
    create_provider_account,
    get_binding,
    list_account_repositories,
    serialize_binding,
    serialize_provider_account,
    set_binding_sync_enabled,
    unbind_repository,
)


def _gitlab_host() -> str:
    return (getattr(settings, "GITLAB_HOST", "") or "https://gitlab.com").rstrip("/")


def _error_response(exc: Exception) -> Response:
    if isinstance(exc, GitProviderAuthError):
        return Response({"error": str(exc) or "Provider rejected this credential"}, status=status.HTTP_401_UNAUTHORIZED)
    if isinstance(exc, GitProviderPermissionError):
        return Response({"error": str(exc) or "Provider credential lacks permission"}, status=status.HTTP_403_FORBIDDEN)
    if isinstance(exc, GitProviderNotFoundError):
        return Response({"error": str(exc) or "Repository not found or inaccessible"}, status=status.HTTP_404_NOT_FOUND)
    if isinstance(exc, GitIntegrationError):
        return Response({"error": str(exc)}, status=exc.status_code)
    return Response({"error": str(exc)}, status=status.HTTP_400_BAD_REQUEST)


class GitProvidersEndpoint(BaseAPIView):
    @allow_permission(allowed_roles=[ROLE.ADMIN, ROLE.MEMBER], level="WORKSPACE")
    def get(self, request, slug):
        get_object_or_404(Workspace, slug=slug)
        return Response({"providers": provider_payload()}, status=status.HTTP_200_OK)


class GitProviderAccountListCreateEndpoint(BaseAPIView):
    @allow_permission(allowed_roles=[ROLE.ADMIN, ROLE.MEMBER], level="WORKSPACE")
    def get(self, request, slug):
        workspace = get_object_or_404(Workspace, slug=slug)
        accounts = GitProviderAccount.objects.filter(workspace=workspace).order_by("provider", "host_url", "display_name")
        return Response({"accounts": [serialize_provider_account(account) for account in accounts]}, status=status.HTTP_200_OK)

    @allow_permission(allowed_roles=[ROLE.ADMIN], level="WORKSPACE")
    def post(self, request, slug):
        workspace = get_object_or_404(Workspace, slug=slug)
        provider = (request.data.get("provider") or "").strip().lower()
        if provider not in {"github", "gitlab"}:
            return Response({"error": "provider must be github or gitlab"}, status=status.HTTP_400_BAD_REQUEST)
        token = (request.data.get("token") or "").strip()
        if not token:
            return Response({"error": "token is required"}, status=status.HTTP_400_BAD_REQUEST)
        auth_type = (request.data.get("auth_type") or "pat").strip()
        host_url = (request.data.get("host_url") or ("https://github.com" if provider == "github" else _gitlab_host())).rstrip("/")
        try:
            account = create_provider_account(
                workspace=workspace,
                actor=request.user,
                provider=provider,
                host_url=host_url,
                auth_type=auth_type,
                token=token,
            )
        except Exception as exc:
            return _error_response(exc)
        return Response(serialize_provider_account(account), status=status.HTTP_201_CREATED)


class GitProviderAccountDetailEndpoint(BaseAPIView):
    @allow_permission(allowed_roles=[ROLE.ADMIN, ROLE.MEMBER], level="WORKSPACE")
    def get(self, request, slug, account_id):
        account = get_object_or_404(GitProviderAccount, id=account_id, workspace__slug=slug)
        return Response(serialize_provider_account(account), status=status.HTTP_200_OK)

    @allow_permission(allowed_roles=[ROLE.ADMIN], level="WORKSPACE")
    def delete(self, request, slug, account_id):
        account = get_object_or_404(GitProviderAccount, id=account_id, workspace__slug=slug)
        account.status = GitProviderAccount.Status.REVOKED
        account.last_check_error = "Provider account disconnected"
        account.credential_config = {**(account.credential_config or {}), "token": ""}
        account.save(update_fields=["status", "last_check_error", "credential_config", "updated_at"])
        account.repository_bindings.update(
            is_sync_enabled=False,
            last_sync_error="Provider account disconnected",
        )
        return Response({"connected": False}, status=status.HTTP_200_OK)


class GitProviderAccountReposEndpoint(BaseAPIView):
    @allow_permission(allowed_roles=[ROLE.ADMIN, ROLE.MEMBER], level="WORKSPACE")
    def get(self, request, slug, account_id):
        account = get_object_or_404(GitProviderAccount, id=account_id, workspace__slug=slug)
        try:
            page = max(1, int(request.query_params.get("page", "1")))
        except ValueError:
            page = 1
        try:
            payload = list_account_repositories(account, page=page)
        except Exception as exc:
            return _error_response(exc)
        return Response(payload, status=status.HTTP_200_OK)


class GitProjectRepositoryEndpoint(BaseAPIView):
    @allow_permission(allowed_roles=[ROLE.ADMIN, ROLE.MEMBER, ROLE.GUEST])
    def get(self, request, slug, project_id):
        binding = get_binding(workspace_slug=slug, project_id=project_id)
        if binding is None:
            return Response({"bound": False}, status=status.HTTP_200_OK)
        return Response(serialize_binding(binding), status=status.HTTP_200_OK)

    @allow_permission(allowed_roles=[ROLE.ADMIN])
    def patch(self, request, slug, project_id):
        enabled = request.data.get("enabled")
        if not isinstance(enabled, bool):
            return Response({"error": "enabled must be boolean"}, status=status.HTTP_400_BAD_REQUEST)
        try:
            binding = set_binding_sync_enabled(workspace_slug=slug, project_id=project_id, enabled=enabled)
        except Exception as exc:
            return _error_response(exc)
        return Response(serialize_binding(binding), status=status.HTTP_200_OK)

    @allow_permission(allowed_roles=[ROLE.ADMIN])
    def delete(self, request, slug, project_id):
        unbind_repository(workspace_slug=slug, project_id=project_id)
        return Response({"bound": False}, status=status.HTTP_200_OK)


class GitProjectRepositoryBindEndpoint(BaseAPIView):
    @allow_permission(allowed_roles=[ROLE.ADMIN])
    def post(self, request, slug, project_id):
        repo_url = (request.data.get("repo_url") or "").strip()
        if not repo_url:
            return Response({"error": "repo_url is required"}, status=status.HTTP_400_BAD_REQUEST)
        try:
            binding, clone_url = bind_repository(
                workspace_slug=slug,
                project_id=project_id,
                actor=request.user,
                raw_url=repo_url,
                provider_account_id=request.data.get("provider_account_id"),
            )
        except Exception as exc:
            return _error_response(exc)
        payload = serialize_binding(binding)
        payload["repo_url"] = clone_url
        return Response(payload, status=status.HTTP_201_CREATED)
