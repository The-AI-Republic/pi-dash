# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""GitHub integration HTTP surface.

Workspace-level: connect (PAT in), repos browse, soft-disconnect.
Project-level:   bind, unbind, sync-toggle, status.

See .ai_design/github_sync/design.md §6.1 and §6.2.
"""

from __future__ import annotations

from django.conf import settings
from django.db import transaction
from django.utils import timezone
from rest_framework import status
from rest_framework.response import Response

from pi_dash.app.permissions import ROLE, allow_permission
from pi_dash.app.views.base import BaseAPIView
from pi_dash.db.models import (
    APIToken,
    GithubIssueSync,
    GithubRepository,
    GithubRepositorySync,
    Integration,
    IssueComment,
    Label,
    Project,
    Workspace,
    WorkspaceIntegration,
)
from pi_dash.license.utils.encryption import decrypt_data, encrypt_data
from pi_dash.utils.exception_logger import log_exception
from pi_dash.utils.github_client import (
    GithubAuthError,
    GithubClient,
    GithubNotFoundError,
    GithubPermissionError,
)


def _feature_enabled() -> bool:
    return getattr(settings, "GITHUB_SYNC_ENABLED", True)


def _disabled_response() -> Response:
    return Response({"error": "GitHub integration is disabled on this instance"}, status=status.HTTP_404_NOT_FOUND)


def _get_or_create_github_integration() -> Integration:
    integration, _ = Integration.objects.get_or_create(
        provider="github",
        defaults={
            "title": "GitHub",
            "verified": True,
            "description": {"summary": "Mirror GitHub issues into Pi Dash projects."},
        },
    )
    return integration


def _get_workspace_integration(workspace: Workspace) -> WorkspaceIntegration | None:
    integration = Integration.objects.filter(provider="github").first()
    if integration is None:
        return None
    return WorkspaceIntegration.objects.filter(workspace=workspace, integration=integration).first()


def _serialize_repo(gh_repo: dict) -> dict:
    owner = (gh_repo.get("owner") or {}).get("login") or ""
    return {
        "id": gh_repo.get("id"),
        "owner": owner,
        "name": gh_repo.get("name") or "",
        "full_name": gh_repo.get("full_name") or "",
        "default_branch": gh_repo.get("default_branch") or "",
        "private": bool(gh_repo.get("private", False)),
    }


# --------------------------------------------------------------------- workspace


class GithubIntegrationConnectEndpoint(BaseAPIView):
    """POST /workspaces/<slug>/integrations/github/connect/"""

    @allow_permission(allowed_roles=[ROLE.ADMIN], level="WORKSPACE")
    def post(self, request, slug):
        if not _feature_enabled():
            return _disabled_response()

        token = (request.data.get("token") or "").strip()
        if not token:
            return Response({"error": "GitHub PAT required"}, status=status.HTTP_400_BAD_REQUEST)

        try:
            user_info = GithubClient(token=token).get_authenticated_user()
        except GithubAuthError:
            return Response({"error": "GitHub rejected this token"}, status=status.HTTP_401_UNAUTHORIZED)
        except (GithubPermissionError, GithubNotFoundError) as e:
            return Response({"error": f"GitHub error: {e}"}, status=status.HTTP_400_BAD_REQUEST)
        except Exception as e:
            log_exception(e)
            return Response({"error": "Failed to verify GitHub credential"}, status=status.HTTP_502_BAD_GATEWAY)

        workspace = Workspace.objects.get(slug=slug)
        integration = _get_or_create_github_integration()

        with transaction.atomic():
            wi = WorkspaceIntegration.objects.filter(workspace=workspace, integration=integration).first()
            if wi is None:
                api_token = APIToken.objects.create(
                    user=request.user,
                    workspace=workspace,
                    user_type=1,
                    label=f"github-integration-{workspace.id}",
                    description="GitHub integration credential",
                )
                wi = WorkspaceIntegration.objects.create(
                    workspace=workspace,
                    actor=request.user,
                    integration=integration,
                    api_token=api_token,
                    config={},
                )
            wi.config = {
                "auth_type": "pat",
                "token": encrypt_data(token),
                "github_user_login": user_info.get("login") or "",
                "verified_at": timezone.now().isoformat(),
            }
            wi.save(update_fields=["config"])

        return Response(
            {
                "connected": True,
                "github_user_login": wi.config.get("github_user_login"),
                "verified_at": wi.config.get("verified_at"),
            },
            status=status.HTTP_200_OK,
        )


class GithubIntegrationDisconnectEndpoint(BaseAPIView):
    """POST /workspaces/<slug>/integrations/github/disconnect/

    Soft-disconnect: clears the credential on `WorkspaceIntegration.config`,
    flips dependent `GithubRepositorySync.is_sync_enabled` to False, leaves
    rows in place to avoid the cascade-delete trap. See §6.1.
    """

    @allow_permission(allowed_roles=[ROLE.ADMIN], level="WORKSPACE")
    def post(self, request, slug):
        if not _feature_enabled():
            return _disabled_response()

        workspace = Workspace.objects.get(slug=slug)
        wi = _get_workspace_integration(workspace)
        if wi is None:
            return Response({"connected": False}, status=status.HTTP_200_OK)

        with transaction.atomic():
            config = wi.config or {}
            config["token"] = ""
            config["disconnected_at"] = timezone.now().isoformat()
            wi.config = config
            wi.save(update_fields=["config"])
            GithubRepositorySync.objects.filter(workspace_integration=wi).update(
                is_sync_enabled=False,
                last_sync_error="Workspace GitHub integration disconnected",
            )
        return Response({"connected": False}, status=status.HTTP_200_OK)


class GithubIntegrationStatusEndpoint(BaseAPIView):
    """GET /workspaces/<slug>/integrations/github/

    Lets the UI tell connected vs. disconnected without parsing the connect
    response. Cheap; no GitHub call.
    """

    @allow_permission(allowed_roles=[ROLE.ADMIN, ROLE.MEMBER], level="WORKSPACE")
    def get(self, request, slug):
        if not _feature_enabled():
            return _disabled_response()
        workspace = Workspace.objects.get(slug=slug)
        wi = _get_workspace_integration(workspace)
        if wi is None or not (wi.config or {}).get("token"):
            return Response({"connected": False}, status=status.HTTP_200_OK)
        return Response(
            {
                "connected": True,
                "github_user_login": wi.config.get("github_user_login"),
                "verified_at": wi.config.get("verified_at"),
            },
            status=status.HTTP_200_OK,
        )


class GithubIntegrationReposEndpoint(BaseAPIView):
    """GET /workspaces/<slug>/integrations/github/repos/?page=N

    Paginated browse of repos visible to the connected PAT. See §6.1 — the
    `affiliation` filter is required to surface org repos.
    """

    @allow_permission(allowed_roles=[ROLE.ADMIN, ROLE.MEMBER], level="WORKSPACE")
    def get(self, request, slug):
        if not _feature_enabled():
            return _disabled_response()
        workspace = Workspace.objects.get(slug=slug)
        wi = _get_workspace_integration(workspace)
        if wi is None:
            return Response({"error": "GitHub not connected"}, status=status.HTTP_404_NOT_FOUND)

        token = decrypt_data((wi.config or {}).get("token") or "")
        if not token:
            return Response({"error": "GitHub credential is missing"}, status=status.HTTP_409_CONFLICT)

        try:
            page = max(1, int(request.query_params.get("page", "1")))
        except ValueError:
            page = 1

        try:
            repos, has_next = GithubClient(token=token).list_user_repos(page=page)
        except GithubAuthError:
            return Response({"error": "GitHub token rejected"}, status=status.HTTP_401_UNAUTHORIZED)
        except Exception as e:
            log_exception(e)
            return Response({"error": "Failed to list repositories"}, status=status.HTTP_502_BAD_GATEWAY)

        return Response(
            {
                "repos": [_serialize_repo(r) for r in repos],
                "page": page,
                "has_next_page": has_next,
            },
            status=status.HTTP_200_OK,
        )


# --------------------------------------------------------------------- project


class GithubProjectBindEndpoint(BaseAPIView):
    """POST /workspaces/<slug>/projects/<id>/github/bind/

    Body: { repository_id: int, owner: str, name: str, url: str }
    """

    @allow_permission(allowed_roles=[ROLE.ADMIN])
    def post(self, request, slug, project_id):
        if not _feature_enabled():
            return _disabled_response()

        # Parse + validate input.
        try:
            repository_id = int(request.data.get("repository_id"))
        except (TypeError, ValueError):
            return Response({"error": "repository_id (int) is required"}, status=status.HTTP_400_BAD_REQUEST)
        owner = (request.data.get("owner") or "").strip()
        name = (request.data.get("name") or "").strip()
        url = (request.data.get("url") or "").strip()
        if not owner or not name:
            return Response({"error": "owner and name are required"}, status=status.HTTP_400_BAD_REQUEST)

        workspace = Workspace.objects.get(slug=slug)
        project = Project.objects.get(pk=project_id, workspace=workspace)
        wi = _get_workspace_integration(workspace)
        if wi is None or not (wi.config or {}).get("token"):
            return Response({"error": "Workspace GitHub integration is not connected"}, status=status.HTTP_409_CONFLICT)

        # Precondition (§6.2): one binding per project.
        if GithubRepositorySync.objects.filter(project=project).exists():
            return Response(
                {"error": "Project already has a GitHub binding; unbind first to change repos"},
                status=status.HTTP_409_CONFLICT,
            )

        # Verify (owner, name, repository_id) consistency upstream.
        token = decrypt_data(wi.config.get("token") or "")
        try:
            verified = GithubClient(token=token).get_repo(owner, name)
        except GithubNotFoundError:
            return Response({"error": "Repository not found or token has no access"}, status=status.HTTP_404_NOT_FOUND)
        except GithubAuthError:
            return Response({"error": "GitHub token rejected"}, status=status.HTTP_401_UNAUTHORIZED)
        except Exception as e:
            log_exception(e)
            return Response({"error": "Failed to verify repository"}, status=status.HTTP_502_BAD_GATEWAY)

        if int(verified.get("id") or 0) != repository_id:
            return Response(
                {"error": "repository_id does not match the upstream repo at owner/name"},
                status=status.HTTP_400_BAD_REQUEST,
            )

        with transaction.atomic():
            repo, _ = GithubRepository.objects.get_or_create(
                project=project,
                repository_id=repository_id,
                defaults={
                    "name": name,
                    "owner": owner,
                    "url": url or verified.get("html_url") or "",
                    "workspace_id": workspace.id,
                    "config": {},
                },
            )
            label, _ = Label.objects.get_or_create(
                project=project,
                name="github",
                defaults={"workspace_id": workspace.id, "color": "#1f2328"},
            )
            sync = GithubRepositorySync.objects.create(
                project=project,
                workspace_id=workspace.id,
                repository=repo,
                workspace_integration=wi,
                actor=request.user,
                label=label,
                credentials={},
                is_sync_enabled=False,
            )

        return Response(
            {
                "id": str(sync.id),
                "repository": _serialize_repo(verified),
                "is_sync_enabled": False,
                "last_synced_at": None,
                "last_sync_error": "",
            },
            status=status.HTTP_201_CREATED,
        )


class GithubProjectStatusEndpoint(BaseAPIView):
    """GET / DELETE / PATCH on /workspaces/<slug>/projects/<id>/github/

    GET    → current binding status (or {bound: False}).
    DELETE → unbind; cascade releases the §6.8 lock.
    PATCH  → toggle `is_sync_enabled`; body { enabled: bool }.
    """

    @allow_permission(allowed_roles=[ROLE.ADMIN, ROLE.MEMBER, ROLE.GUEST])
    def get(self, request, slug, project_id):
        if not _feature_enabled():
            return _disabled_response()
        sync = (
            GithubRepositorySync.objects
            .filter(project_id=project_id, workspace__slug=slug)
            .select_related("repository")
            .first()
        )
        if sync is None:
            return Response({"bound": False}, status=status.HTTP_200_OK)
        return Response(
            {
                "bound": True,
                "id": str(sync.id),
                "repository": {
                    "id": sync.repository.repository_id,
                    "owner": sync.repository.owner,
                    "name": sync.repository.name,
                    "url": sync.repository.url,
                },
                "is_sync_enabled": sync.is_sync_enabled,
                "last_synced_at": sync.last_synced_at.isoformat() if sync.last_synced_at else None,
                "last_sync_error": sync.last_sync_error,
            },
            status=status.HTTP_200_OK,
        )

    @allow_permission(allowed_roles=[ROLE.ADMIN])
    def patch(self, request, slug, project_id):
        if not _feature_enabled():
            return _disabled_response()
        enabled = request.data.get("enabled")
        if not isinstance(enabled, bool):
            return Response({"error": "enabled (bool) is required"}, status=status.HTTP_400_BAD_REQUEST)
        sync = GithubRepositorySync.objects.filter(project_id=project_id, workspace__slug=slug).first()
        if sync is None:
            return Response({"error": "No GitHub binding for this project"}, status=status.HTTP_404_NOT_FOUND)
        sync.is_sync_enabled = enabled
        sync.save(update_fields=["is_sync_enabled"])
        return Response({"is_sync_enabled": enabled}, status=status.HTTP_200_OK)

    @allow_permission(allowed_roles=[ROLE.ADMIN])
    def delete(self, request, slug, project_id):
        if not _feature_enabled():
            return _disabled_response()
        sync = GithubRepositorySync.objects.filter(project_id=project_id, workspace__slug=slug).first()
        if sync is None:
            return Response({"bound": False}, status=status.HTTP_200_OK)
        # Cascade deletes GithubIssueSync/GithubCommentSync, releasing the §6.8
        # lock on surviving Issue/IssueComment rows.
        sync.delete()
        return Response({"bound": False}, status=status.HTTP_200_OK)
