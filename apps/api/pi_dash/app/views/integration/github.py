# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""GitHub integration HTTP surface.

Workspace-level: connect (PAT in), repos browse, soft-disconnect.
Project-level:   bind, unbind, sync-toggle, status.

See .ai_design/github_sync/design.md §6.1 and §6.2.
"""

from __future__ import annotations

import json
import logging
import secrets
import uuid
from datetime import timedelta
from urllib.parse import urlencode

from django.conf import settings
from django.db import transaction
from django.http import HttpResponseRedirect
from django.shortcuts import get_object_or_404
from django.utils import timezone
from rest_framework.permissions import AllowAny
from rest_framework import status
from rest_framework.response import Response

from pi_dash.app.permissions import ROLE, allow_permission
from pi_dash.app.views.base import BaseAPIView
from pi_dash.db.models import (
    APIToken,
    GithubAppInstallation,
    GithubAppInstallSession,
    GithubCommentSync,
    GithubIssueSync,
    GithubPullRequestLink,
    GithubRepository,
    GithubRepositorySync,
    GithubWebhookDelivery,
    Integration,
    Label,
    Project,
    Workspace,
    WorkspaceMember,
    WorkspaceIntegration,
)
from pi_dash.license.utils.encryption import decrypt_data, encrypt_data
from pi_dash.utils.exception_logger import log_exception
from pi_dash.utils.github_app_auth import (
    GithubAppAuthError,
    GithubAppConfigError,
    exchange_user_code,
    get_github_app_config,
    get_installation,
    parse_github_datetime,
    require_github_app_config,
    revoke_installation_cache,
    verify_user_can_access_installation,
    verify_webhook_signature,
)
from pi_dash.utils.github_client import (
    GithubAuthError,
    GithubClient,
    GithubNotFoundError,
    GithubPermissionError,
    parse_github_repo_url,
    pr_snapshot_from_payload,
)

logger = logging.getLogger(__name__)


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


def _get_or_create_workspace_integration(workspace: Workspace, actor) -> WorkspaceIntegration:
    integration = _get_or_create_github_integration()
    wi = WorkspaceIntegration.objects.filter(workspace=workspace, integration=integration).first()
    if wi is not None:
        return wi
    api_token = APIToken.objects.create(
        user=actor,
        workspace=workspace,
        user_type=1,
        is_active=False,
        label=f"github-integration-{workspace.id}",
        description="GitHub integration FK shim — not for auth",
    )
    return WorkspaceIntegration.objects.create(
        workspace=workspace,
        actor=actor,
        integration=integration,
        api_token=api_token,
        config={},
    )


def _is_workspace_admin(user, workspace: Workspace) -> bool:
    return WorkspaceMember.objects.filter(
        member=user,
        workspace=workspace,
        role=ROLE.ADMIN.value,
        is_active=True,
    ).exists()


def _redirect_to_profile_integrations(params: dict[str, str]) -> HttpResponseRedirect:
    base = (getattr(settings, "WEB_URL", None) or getattr(settings, "APP_BASE_URL", None) or "").rstrip("/")
    path = "/settings/profile/integrations/"
    query = urlencode(params)
    url = f"{base}{path}" if base else path
    if query:
        url = f"{url}?{query}"
    return HttpResponseRedirect(url)


def _serialize_app_installation(app_installation: GithubAppInstallation | None) -> dict:
    if app_installation is None:
        return {"connected": False}
    return {
        "connected": True,
        "installation_id": app_installation.installation_id,
        "account_login": app_installation.account_login,
        "account_type": app_installation.account_type,
        "repository_selection": app_installation.repository_selection,
        "repository_count": app_installation.repository_count,
        "permissions": app_installation.permissions,
        "events": app_installation.events,
        "installed_at": app_installation.installed_at.isoformat() if app_installation.installed_at else None,
        "suspended_at": app_installation.suspended_at.isoformat() if app_installation.suspended_at else None,
        "verified_at": app_installation.verified_at.isoformat() if app_installation.verified_at else None,
        "last_checked_at": app_installation.last_checked_at.isoformat() if app_installation.last_checked_at else None,
        "last_check_error": app_installation.last_check_error,
    }


def _lazy_cleanup_install_sessions() -> None:
    now = timezone.now()
    try:
        expired_count = GithubAppInstallSession.objects.filter(
            status=GithubAppInstallSession.Status.STARTED,
            expires_at__lt=now,
        ).update(status=GithubAppInstallSession.Status.EXPIRED, error="Install session expired")
        deleted_count, _ = GithubAppInstallSession.objects.filter(
            status__in=[
                GithubAppInstallSession.Status.COMPLETED,
                GithubAppInstallSession.Status.EXPIRED,
                GithubAppInstallSession.Status.FAILED,
            ],
            updated_at__lt=now - timedelta(days=7),
        ).delete(soft=False)
        if expired_count or deleted_count:
            logger.info("GitHub App install session cleanup: expired=%s, deleted=%s", expired_count, deleted_count)
    except Exception as e:
        log_exception(e)


def _refresh_app_installation(
    app_installation: GithubAppInstallation,
    *,
    raise_on_error: bool = False,
    extra_update_fields: list[str] | None = None,
) -> GithubAppInstallation:
    now = timezone.now()
    update_fields = [
        "repository_count",
        "verified_at",
        "last_checked_at",
        "last_check_error",
        "updated_at",
    ]
    if extra_update_fields:
        update_fields.extend(field for field in extra_update_fields if field not in update_fields)

    try:
        _, _, repository_count = (
            GithubClient.for_installation(app_installation.installation_id).list_installation_repositories()
        )
        app_installation.repository_count = repository_count
        app_installation.verified_at = now
        app_installation.last_checked_at = now
        app_installation.last_check_error = ""
    except Exception as e:
        app_installation.last_checked_at = now
        app_installation.last_check_error = str(e)[:2000]
        log_exception(e)
        app_installation.save(update_fields=update_fields)
        if raise_on_error:
            raise GithubAppAuthError("GitHub App connection check failed") from e
        return app_installation

    app_installation.save(update_fields=update_fields)
    return app_installation


def _upsert_app_installation(
    workspace: Workspace,
    actor,
    installation: dict,
    *,
    require_verified: bool = False,
) -> GithubAppInstallation:
    account = installation.get("account") or {}
    installation_id = int(installation.get("id") or 0)
    if not installation_id:
        raise GithubAppAuthError("GitHub installation response did not include an id")
    defaults = {
        "account_login": account.get("login") or "",
        "account_type": account.get("type") or GithubAppInstallation.AccountType.UNKNOWN,
        "repository_selection": installation.get("repository_selection")
        or GithubAppInstallation.RepositorySelection.SELECTED,
        "repository_count": int(installation.get("repository_count") or 0),
        "permissions": installation.get("permissions") or {},
        "events": installation.get("events") or [],
        "installed_at": parse_github_datetime(installation.get("created_at")),
        "suspended_at": parse_github_datetime(installation.get("suspended_at")),
        "last_check_error": "",
    }
    with transaction.atomic():
        wi = _get_or_create_workspace_integration(workspace, actor)
        existing = GithubAppInstallation.objects.filter(installation_id=installation_id).select_related(
            "workspace_integration__workspace"
        ).first()
        if existing is not None and existing.workspace_integration_id != wi.id:
            raise GithubAppAuthError(
                f"GitHub installation is already connected to workspace {existing.workspace_integration.workspace.slug}"
            )
        app_installation, _ = GithubAppInstallation.objects.update_or_create(
            workspace_integration=wi,
            defaults={"installation_id": installation_id, **defaults},
        )
        return _refresh_app_installation(app_installation, raise_on_error=require_verified)


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

        workspace = get_object_or_404(Workspace, slug=slug)
        integration = _get_or_create_github_integration()

        with transaction.atomic():
            wi = WorkspaceIntegration.objects.filter(workspace=workspace, integration=integration).first()
            if wi is None:
                # The WorkspaceIntegration.api_token FK is required by the
                # schema, but the real GitHub credential lives encrypted in
                # `config["token"]`. Mint an INACTIVE token solely to satisfy
                # the FK — it must not authenticate API requests, since it
                # would otherwise grant the connecting user's full API surface
                # to anyone who could read this row. See review of #65.
                api_token = APIToken.objects.create(
                    user=request.user,
                    workspace=workspace,
                    user_type=1,
                    is_active=False,
                    label=f"github-integration-{workspace.id}",
                    description="GitHub integration FK shim — not for auth",
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

        workspace = get_object_or_404(Workspace, slug=slug)
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
        workspace = get_object_or_404(Workspace, slug=slug)
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
        workspace = get_object_or_404(Workspace, slug=slug)
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


# --------------------------------------------------------------------- github app


class GithubAppStatusEndpoint(BaseAPIView):
    """GET /users/me/integrations/github/app/"""

    def get(self, request):
        if not _feature_enabled():
            return _disabled_response()
        _lazy_cleanup_install_sessions()

        config = get_github_app_config()
        configured = bool(
            config.get("app_id")
            and config.get("app_slug")
            and config.get("private_key")
            and config.get("webhook_secret")
            and config.get("client_id")
            and config.get("client_secret")
        )
        memberships = (
            WorkspaceMember.objects.filter(member=request.user, role=ROLE.ADMIN.value, is_active=True)
            .select_related("workspace")
            .order_by("workspace__name")
        )
        workspace_payload = []
        for membership in memberships:
            wi = _get_workspace_integration(membership.workspace)
            app_installation = None
            if wi is not None:
                app_installation = getattr(wi, "github_app_installation", None)
            workspace_payload.append(
                {
                    "id": str(membership.workspace.id),
                    "slug": membership.workspace.slug,
                    "name": membership.workspace.name,
                    "github_app": _serialize_app_installation(app_installation),
                }
            )

        return Response(
            {
                "configured": configured,
                "app_slug": config.get("app_slug") or "",
                "workspaces": workspace_payload,
            },
            status=status.HTTP_200_OK,
        )


class GithubAppInstallStartEndpoint(BaseAPIView):
    """POST /users/me/integrations/github/app/install/"""

    def post(self, request):
        if not _feature_enabled():
            return _disabled_response()
        _lazy_cleanup_install_sessions()
        try:
            config = require_github_app_config(oauth=True, webhook=True)
        except GithubAppConfigError as e:
            return Response({"error": str(e)}, status=status.HTTP_409_CONFLICT)

        workspace_slug = (request.data.get("workspace_slug") or "").strip()
        if not workspace_slug:
            return Response({"error": "workspace_slug is required"}, status=status.HTTP_400_BAD_REQUEST)
        workspace = get_object_or_404(Workspace, slug=workspace_slug)
        if not _is_workspace_admin(request.user, workspace):
            return Response({"error": "You must be a workspace admin to install the GitHub App"}, status=status.HTTP_403_FORBIDDEN)

        state = secrets.token_urlsafe(32)
        install_session = GithubAppInstallSession.objects.create(
            state=state,
            workspace=workspace,
            actor=request.user,
            expires_at=timezone.now() + timedelta(minutes=15),
        )
        install_url = f"https://github.com/apps/{config['app_slug']}/installations/new?{urlencode({'state': state})}"
        return Response(
            {
                "state": install_session.state,
                "expires_at": install_session.expires_at.isoformat(),
                "install_url": install_url,
            },
            status=status.HTTP_201_CREATED,
        )


class GithubAppRefreshEndpoint(BaseAPIView):
    """POST /users/me/integrations/github/app/refresh/"""

    def post(self, request):
        if not _feature_enabled():
            return _disabled_response()
        workspace_slug = (request.data.get("workspace_slug") or "").strip()
        if not workspace_slug:
            return Response({"error": "workspace_slug is required"}, status=status.HTTP_400_BAD_REQUEST)
        workspace = get_object_or_404(Workspace, slug=workspace_slug)
        if not _is_workspace_admin(request.user, workspace):
            return Response({"error": "You must be a workspace admin to refresh this connection"}, status=status.HTTP_403_FORBIDDEN)
        wi = _get_workspace_integration(workspace)
        app_installation = getattr(wi, "github_app_installation", None) if wi else None
        if app_installation is None:
            return Response({"error": "GitHub App is not installed for this workspace"}, status=status.HTTP_404_NOT_FOUND)
        try:
            app_installation = _refresh_app_installation(app_installation, raise_on_error=True)
        except GithubAppAuthError:
            return Response(
                {"error": app_installation.last_check_error or "Failed to verify GitHub App installation"},
                status=status.HTTP_502_BAD_GATEWAY,
            )
        return Response(_serialize_app_installation(app_installation), status=status.HTTP_200_OK)


class GithubAppCallbackEndpoint(BaseAPIView):
    """GET /integrations/github/app/callback/"""

    permission_classes = [AllowAny]

    def get(self, request):
        if not _feature_enabled():
            return _redirect_to_profile_integrations({"github_app": "disabled"})
        # GitHub redirects the browser here; if the Pi Dash session isn't present
        # (logged out / different browser), redirect with a friendly error instead
        # of letting DRF return a bare 403. The actor check below is the real guard.
        if not request.user.is_authenticated:
            return _redirect_to_profile_integrations({"github_app": "error", "error": "login_required"})
        _lazy_cleanup_install_sessions()

        state = request.GET.get("state") or ""
        code = request.GET.get("code") or ""
        installation_id_raw = request.GET.get("installation_id") or ""
        if not state:
            return _redirect_to_profile_integrations({"github_app": "error", "error": "missing_state"})

        install_session = GithubAppInstallSession.objects.filter(state=state).select_related("workspace", "actor").first()
        if install_session is None:
            return _redirect_to_profile_integrations({"github_app": "error", "error": "unknown_state"})

        def fail(error: str):
            install_session.status = GithubAppInstallSession.Status.FAILED
            install_session.error = error
            install_session.save(update_fields=["status", "error", "updated_at"])
            return _redirect_to_profile_integrations({"github_app": "error", "error": error})

        if install_session.status != GithubAppInstallSession.Status.STARTED:
            return _redirect_to_profile_integrations({"github_app": install_session.status})
        if install_session.expires_at <= timezone.now():
            install_session.status = GithubAppInstallSession.Status.EXPIRED
            install_session.error = "Install session expired"
            install_session.save(update_fields=["status", "error", "updated_at"])
            return _redirect_to_profile_integrations({"github_app": "expired"})
        if install_session.actor_id != request.user.id:
            return fail("actor_mismatch")
        if not _is_workspace_admin(request.user, install_session.workspace):
            return fail("workspace_admin_required")
        if not installation_id_raw.isdigit():
            return fail("missing_installation_id")
        if not code:
            return fail("missing_oauth_code")

        installation_id = int(installation_id_raw)
        try:
            user_token = exchange_user_code(code)
            if verify_user_can_access_installation(user_token, installation_id) is None:
                return fail("installation_not_visible_to_user")
            installation = get_installation(installation_id)
            app_installation = _upsert_app_installation(
                install_session.workspace,
                request.user,
                installation,
                require_verified=True,
            )
        except Exception as e:
            log_exception(e)
            return fail("github_verification_failed")

        install_session.installation_id = installation_id
        install_session.account_login = app_installation.account_login
        install_session.status = GithubAppInstallSession.Status.COMPLETED
        install_session.completed_at = timezone.now()
        install_session.error = ""
        install_session.save(
            update_fields=["installation_id", "account_login", "status", "completed_at", "error", "updated_at"]
        )
        return _redirect_to_profile_integrations(
            {
                "github_app": "connected",
                "workspace_slug": install_session.workspace.slug,
            }
        )


def _refresh_pr_links(payload: dict) -> int:
    """Refresh the display snapshot of any `GithubPullRequestLink` matching the
    PR in a `pull_request` webhook. Touches only the link row — never the issue.

    Returns the number of links the PR *matched* (0 when no link is attached,
    which the caller maps to a `skipped` delivery). A matched-but-stale delivery
    still counts as matched — it is a real, processed delivery, just not applied —
    so it is not mislabelled `skipped`. Out-of-order/replayed deliveries are
    ignored via the PR's `updated_at`.
    """
    pull_request = payload.get("pull_request") or {}
    repository = payload.get("repository") or {}
    number = pull_request.get("number")
    owner = ((repository.get("owner") or {}).get("login") or "").lower()
    name = (repository.get("name") or "").lower()
    if not (owner and name and number):
        return 0

    snapshot = pr_snapshot_from_payload(pull_request)
    incoming = snapshot.get("pr_updated_at")

    matched = 0
    for link in GithubPullRequestLink.objects.filter(repo_owner=owner, repo_name=name, pr_number=number):
        matched += 1
        if incoming and link.pr_updated_at and incoming < link.pr_updated_at:
            continue  # stale / out-of-order delivery — matched but not applied
        for field, value in snapshot.items():
            setattr(link, field, value)
        link.save(update_fields=["title", "state", "merged", "draft", "pr_updated_at", "updated_at"])
    return matched


class GithubAppWebhookEndpoint(BaseAPIView):
    """POST /integrations/github/app/webhook/"""

    authentication_classes = []
    permission_classes = [AllowAny]
    # GitHub delivers from shared egress IPs; the default AnonRateThrottle
    # (30/min) would 429 bursts of lifecycle deliveries. Signature is the auth.
    throttle_classes = []

    def post(self, request):
        if not _feature_enabled():
            return _disabled_response()
        try:
            if not verify_webhook_signature(request.body, request.headers.get("X-Hub-Signature-256")):
                return Response({"error": "Invalid signature"}, status=status.HTTP_401_UNAUTHORIZED)
        except GithubAppConfigError as e:
            return Response({"error": str(e)}, status=status.HTTP_409_CONFLICT)

        delivery_id = request.headers.get("X-GitHub-Delivery")
        event = request.headers.get("X-GitHub-Event") or ""
        if not delivery_id:
            return Response({"error": "Missing X-GitHub-Delivery"}, status=status.HTTP_400_BAD_REQUEST)
        try:
            delivery_uuid = uuid.UUID(delivery_id)
        except ValueError:
            return Response({"error": "Invalid X-GitHub-Delivery"}, status=status.HTTP_400_BAD_REQUEST)
        try:
            payload = json.loads(request.body.decode("utf-8") or "{}")
        except json.JSONDecodeError:
            return Response({"error": "Invalid JSON"}, status=status.HTTP_400_BAD_REQUEST)

        installation = payload.get("installation") or {}
        installation_id = installation.get("id")
        action = payload.get("action") or ""
        delivery, created = GithubWebhookDelivery.objects.get_or_create(
            delivery_id=delivery_uuid,
            defaults={
                "event": event,
                "action": action,
                "installation_id": installation_id,
                "payload": payload,
                "status": GithubWebhookDelivery.Status.RECEIVED,
            },
        )
        if not created:
            return Response({"status": delivery.status}, status=status.HTTP_202_ACCEPTED)

        try:
            if event == "ping":
                delivery.status = GithubWebhookDelivery.Status.PROCESSED
            elif event == "pull_request":
                updated = _refresh_pr_links(payload)
                delivery.status = (
                    GithubWebhookDelivery.Status.PROCESSED if updated else GithubWebhookDelivery.Status.SKIPPED
                )
            elif event in {"installation", "installation_repositories"}:
                app_installation = None
                if installation_id:
                    app_installation = GithubAppInstallation.objects.filter(installation_id=installation_id).first()
                if app_installation is None:
                    delivery.status = GithubWebhookDelivery.Status.SKIPPED
                else:
                    if event == "installation" and action in {"deleted", "suspend"}:
                        app_installation.suspended_at = timezone.now()
                        app_installation.last_check_error = "GitHub App installation removed or suspended"
                        app_installation.save(update_fields=["suspended_at", "last_check_error", "updated_at"])
                        revoke_installation_cache(app_installation.installation_id)
                    elif event == "installation" and action == "unsuspend":
                        app_installation.suspended_at = None
                        _refresh_app_installation(app_installation, extra_update_fields=["suspended_at"])
                    elif event == "installation_repositories":
                        _refresh_app_installation(app_installation)
                    delivery.status = GithubWebhookDelivery.Status.PROCESSED
            else:
                delivery.status = GithubWebhookDelivery.Status.SKIPPED
            delivery.processed_at = timezone.now()
            delivery.save(update_fields=["status", "processed_at", "updated_at"])
        except Exception as e:
            log_exception(e)
            delivery.status = GithubWebhookDelivery.Status.FAILED
            delivery.error = str(e)[:2000]
            delivery.processed_at = timezone.now()
            delivery.save(update_fields=["status", "error", "processed_at", "updated_at"])
        return Response({"status": delivery.status}, status=status.HTTP_202_ACCEPTED)


# --------------------------------------------------------------------- project


class GithubProjectBindEndpoint(BaseAPIView):
    """POST /workspaces/<slug>/projects/<id>/github/bind/

    Body: { repo_url: str }

    The General-Settings flow makes Bind double as the "save" for the
    project's `repo_url` field — clicking Bind verifies the URL upstream,
    creates the binding, AND writes `project.repo_url` to the canonical
    `html_url` returned by GitHub. The free-text URL field on the form is
    therefore not persisted independently of Bind, eliminating the prior
    inconsistency where users could set a URL in General Settings that
    contradicted whatever repo was bound in the GitHub tab.

    If the project already has an active binding for a *different* repo,
    Bind acts as Rebind: synchronously soft-deletes the old sync rows
    (matching the unbind cascade-race fix from c54652f8) and creates the
    fresh binding inside the same transaction.
    """

    @allow_permission(allowed_roles=[ROLE.ADMIN])
    def post(self, request, slug, project_id):
        if not _feature_enabled():
            return _disabled_response()

        repo_url = (request.data.get("repo_url") or "").strip()
        if not repo_url:
            return Response({"error": "repo_url is required"}, status=status.HTTP_400_BAD_REQUEST)
        parsed = parse_github_repo_url(repo_url)
        if parsed is None:
            return Response(
                {"error": "Only github.com URLs are supported (e.g. https://github.com/owner/repo)"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        owner, name = parsed

        workspace = get_object_or_404(Workspace, slug=slug)
        project = get_object_or_404(Project, pk=project_id, workspace=workspace)
        wi = _get_workspace_integration(workspace)
        if wi is None or not (wi.config or {}).get("token"):
            return Response({"error": "Workspace GitHub integration is not connected"}, status=status.HTTP_409_CONFLICT)

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

        repository_id = int(verified.get("id") or 0)
        if not repository_id:
            return Response({"error": "GitHub did not return a repository id"}, status=status.HTTP_502_BAD_GATEWAY)
        canonical_url = verified.get("html_url") or f"https://github.com/{owner}/{name}"

        try:
            with transaction.atomic():
                # Rebind: if a binding already exists, hard-delete it so the
                # FK-level UNIQUE on `GithubRepositorySync.repository_id`
                # (auto-generated by the OneToOneField) doesn't trip when the
                # new row points at the same `GithubRepository`. The
                # conditional unique constraint added by migration 0127 only
                # covers the (project) column; the OneToOne uniqueness on
                # repository is unconditional and a soft-deleted row would
                # still occupy that slot. Postgres FK cascade hard-deletes
                # dependent GithubIssueSync / GithubCommentSync rows too.
                existing = GithubRepositorySync.objects.filter(project=project).first()
                if existing is not None:
                    existing.delete(soft=False)

                repo, _ = GithubRepository.objects.get_or_create(
                    project=project,
                    repository_id=repository_id,
                    defaults={
                        "name": name,
                        "owner": owner,
                        "url": canonical_url,
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
                # Bind doubles as save: persist canonical URL on the project
                # so General Settings shows what's actually bound.
                if project.repo_url != canonical_url:
                    project.repo_url = canonical_url
                    project.save(update_fields=["repo_url"])
        except IntegrityError:
            return Response(
                {"error": "Could not bind: concurrent change detected, please retry"},
                status=status.HTTP_409_CONFLICT,
            )

        return Response(
            {
                "id": str(sync.id),
                "repository": _serialize_repo(verified),
                "is_sync_enabled": False,
                "last_synced_at": None,
                "last_sync_error": "",
                "repo_url": canonical_url,
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
        with transaction.atomic():
            # Synchronously soft-delete the dependent sync-tracking rows so
            # the §6.8 lock predicate (which checks GithubIssueSync /
            # GithubCommentSync existence via the default soft-delete-aware
            # manager) releases atomically with the unbind. SoftDeleteModel's
            # cascade is async (queues a Celery task); without this pre-step,
            # there's a race window where the user just unbound but their
            # next edit on a freshly-mirrored issue still trips the lock.
            GithubCommentSync.objects.filter(issue_sync__repository_sync=sync).delete()
            GithubIssueSync.objects.filter(repository_sync=sync).delete()
            sync.delete()
        return Response({"bound": False}, status=status.HTTP_200_OK)
