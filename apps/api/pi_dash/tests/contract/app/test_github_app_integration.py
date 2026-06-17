# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import json
from datetime import timedelta
from uuid import uuid4
from unittest.mock import patch

import pytest
from django.urls import reverse
from django.utils import timezone
from rest_framework import status
from rest_framework.test import APIClient

from pi_dash.app.views.integration.github import _get_or_create_workspace_integration
from pi_dash.db.models import (
    GithubAppInstallation,
    GithubAppInstallSession,
    GithubWebhookDelivery,
    WorkspaceIntegration,
    WorkspaceMember,
)
from pi_dash.license.models import Instance, InstanceAdmin, InstanceConfiguration
from pi_dash.license.utils.encryption import decrypt_data, encrypt_data
from pi_dash.tests.factories import UserFactory, WorkspaceFactory, WorkspaceMemberFactory
from pi_dash.utils.github_app_auth import GithubAppAuthError


pytestmark = [pytest.mark.contract, pytest.mark.django_db]


@pytest.fixture(autouse=True)
def github_app_test_settings(settings):
    settings.REST_FRAMEWORK = {
        **settings.REST_FRAMEWORK,
        "DEFAULT_THROTTLE_CLASSES": (),
    }


def _github_app_config() -> dict[str, str]:
    return {
        "app_id": "123",
        "app_slug": "pi-dash-test",
        "private_key": "private-key",
        "webhook_secret": "webhook-secret",
        "client_id": "client-id",
        "client_secret": "client-secret",
    }


def _installation_payload(installation_id: int = 98765) -> dict:
    return {
        "id": installation_id,
        "account": {"login": "acme-corp", "type": "Organization"},
        "repository_selection": "selected",
        "repository_count": 3,
        "permissions": {"metadata": "read"},
        "events": ["installation", "installation_repositories"],
        "created_at": "2026-06-16T19:00:00Z",
        "suspended_at": None,
    }


def _mark_refreshed(app_installation: GithubAppInstallation, **_kwargs) -> GithubAppInstallation:
    now = timezone.now()
    app_installation.verified_at = now
    app_installation.last_checked_at = now
    app_installation.repository_count = 3
    app_installation.last_check_error = ""
    app_installation.save(
        update_fields=["verified_at", "last_checked_at", "repository_count", "last_check_error", "updated_at"]
    )
    return app_installation


class _FakeInstallationClient:
    def __init__(self, repository_count: int = 3):
        self.repository_count = repository_count

    def list_installation_repositories(self):
        return [], False, self.repository_count


def test_github_app_status_lists_admin_workspaces(session_client, workspace):
    url = reverse("github-app-status")

    with patch("pi_dash.app.views.integration.github.get_github_app_config", return_value=_github_app_config()):
        response = session_client.get(url)

    assert response.status_code == status.HTTP_200_OK
    assert response.data["configured"] is True
    assert response.data["app_slug"] == "pi-dash-test"
    assert response.data["workspaces"] == [
        {
            "id": str(workspace.id),
            "slug": workspace.slug,
            "name": workspace.name,
            "github_app": {"connected": False},
        }
    ]


def test_github_app_status_requires_webhook_secret(session_client):
    config = _github_app_config()
    config["webhook_secret"] = ""

    with patch("pi_dash.app.views.integration.github.get_github_app_config", return_value=config):
        response = session_client.get(reverse("github-app-status"))

    assert response.status_code == status.HTTP_200_OK
    assert response.data["configured"] is False


def test_github_app_status_excludes_non_admin_workspaces(session_client, create_user):
    other_workspace = WorkspaceFactory(owner=create_user, slug="member-only-workspace")
    WorkspaceMemberFactory(workspace=other_workspace, member=create_user, role=15)

    with patch("pi_dash.app.views.integration.github.get_github_app_config", return_value=_github_app_config()):
        response = session_client.get(reverse("github-app-status"))

    assert response.status_code == status.HTTP_200_OK
    assert all(item["slug"] != other_workspace.slug for item in response.data["workspaces"])


def test_github_app_install_start_creates_session(session_client, workspace):
    url = reverse("github-app-install-start")

    with patch("pi_dash.app.views.integration.github.require_github_app_config", return_value=_github_app_config()) as require_config:
        response = session_client.post(url, {"workspace_slug": workspace.slug}, format="json")

    assert response.status_code == status.HTTP_201_CREATED
    require_config.assert_called_once_with(oauth=True, webhook=True)
    assert response.data["install_url"].startswith("https://github.com/apps/pi-dash-test/installations/new?")

    install_session = GithubAppInstallSession.objects.get(state=response.data["state"])
    assert install_session.workspace == workspace
    assert install_session.actor.email == "test@example.com"
    assert install_session.status == GithubAppInstallSession.Status.STARTED
    assert install_session.expires_at > timezone.now()
    assert f"state={install_session.state}" in response.data["install_url"]


def test_github_app_install_start_requires_workspace_admin(session_client, workspace, create_user):
    WorkspaceMember.objects.filter(workspace=workspace, member=create_user).update(role=15)

    with patch("pi_dash.app.views.integration.github.require_github_app_config", return_value=_github_app_config()):
        response = session_client.post(
            reverse("github-app-install-start"),
            {"workspace_slug": workspace.slug},
            format="json",
        )

    assert response.status_code == status.HTTP_403_FORBIDDEN
    assert GithubAppInstallSession.objects.count() == 0


def test_github_app_callback_verifies_and_binds_installation(session_client, workspace, create_user):
    install_session = GithubAppInstallSession.objects.create(
        state="state-123",
        workspace=workspace,
        actor=create_user,
        expires_at=timezone.now() + timedelta(minutes=15),
    )

    with (
        patch("pi_dash.app.views.integration.github.exchange_user_code", return_value="user-token") as exchange_code,
        patch(
            "pi_dash.app.views.integration.github.verify_user_can_access_installation",
            return_value={"id": 98765},
        ) as verify_user_install,
        patch("pi_dash.app.views.integration.github.get_installation", return_value=_installation_payload()),
        patch("pi_dash.app.views.integration.github._refresh_app_installation", side_effect=_mark_refreshed),
    ):
        response = session_client.get(
            reverse("github-app-callback"),
            {"state": install_session.state, "code": "oauth-code", "installation_id": "98765"},
        )

    assert response.status_code == status.HTTP_302_FOUND
    assert response.headers["Location"].endswith(
        f"/settings/profile/integrations/?github_app=connected&workspace_slug={workspace.slug}"
    )
    exchange_code.assert_called_once_with("oauth-code")
    verify_user_install.assert_called_once_with("user-token", 98765)

    install_session.refresh_from_db()
    assert install_session.status == GithubAppInstallSession.Status.COMPLETED
    assert install_session.installation_id == 98765
    assert install_session.account_login == "acme-corp"

    workspace_integration = WorkspaceIntegration.objects.get(workspace=workspace, integration__provider="github")
    assert workspace_integration.config == {}
    app_installation = GithubAppInstallation.objects.get(workspace_integration=workspace_integration)
    assert app_installation.installation_id == 98765
    assert app_installation.account_login == "acme-corp"
    assert app_installation.repository_count == 3
    assert app_installation.verified_at is not None


def test_github_app_callback_fails_when_connection_check_fails(session_client, workspace, create_user):
    install_session = GithubAppInstallSession.objects.create(
        state="state-refresh-fails",
        workspace=workspace,
        actor=create_user,
        expires_at=timezone.now() + timedelta(minutes=15),
    )

    with (
        patch("pi_dash.app.views.integration.github.exchange_user_code", return_value="user-token"),
        patch("pi_dash.app.views.integration.github.verify_user_can_access_installation", return_value={"id": 98765}),
        patch("pi_dash.app.views.integration.github.get_installation", return_value=_installation_payload()),
        patch(
            "pi_dash.app.views.integration.github._refresh_app_installation",
            side_effect=GithubAppAuthError("token rejected"),
        ),
    ):
        response = session_client.get(
            reverse("github-app-callback"),
            {"state": install_session.state, "code": "oauth-code", "installation_id": "98765"},
        )

    assert response.status_code == status.HTTP_302_FOUND
    assert "github_app=error" in response.headers["Location"]
    assert "error=github_verification_failed" in response.headers["Location"]
    install_session.refresh_from_db()
    assert install_session.status == GithubAppInstallSession.Status.FAILED
    assert GithubAppInstallation.objects.count() == 0
    assert WorkspaceIntegration.objects.filter(workspace=workspace, integration__provider="github").count() == 0


def test_github_app_callback_rejects_actor_mismatch(session_client, workspace):
    other_user = UserFactory(email="other@example.com", username="other-user")
    install_session = GithubAppInstallSession.objects.create(
        state="state-actor-mismatch",
        workspace=workspace,
        actor=other_user,
        expires_at=timezone.now() + timedelta(minutes=15),
    )

    response = session_client.get(
        reverse("github-app-callback"),
        {"state": install_session.state, "code": "oauth-code", "installation_id": "98765"},
    )

    assert response.status_code == status.HTTP_302_FOUND
    assert "github_app=error" in response.headers["Location"]
    assert "error=actor_mismatch" in response.headers["Location"]
    install_session.refresh_from_db()
    assert install_session.status == GithubAppInstallSession.Status.FAILED
    assert GithubAppInstallation.objects.count() == 0


def test_github_app_callback_redirects_when_unauthenticated(api_client, workspace, create_user):
    install_session = GithubAppInstallSession.objects.create(
        state="state-logged-out",
        workspace=workspace,
        actor=create_user,
        expires_at=timezone.now() + timedelta(minutes=15),
    )

    response = api_client.get(
        reverse("github-app-callback"),
        {"state": install_session.state, "code": "oauth-code", "installation_id": "98765"},
    )

    assert response.status_code == status.HTTP_302_FOUND
    assert "github_app=error" in response.headers["Location"]
    assert "error=login_required" in response.headers["Location"]
    install_session.refresh_from_db()
    assert install_session.status == GithubAppInstallSession.Status.STARTED
    assert GithubAppInstallation.objects.count() == 0


def test_github_app_refresh_returns_error_when_connection_check_fails(session_client, workspace, create_user):
    workspace_integration = _get_or_create_workspace_integration(workspace, create_user)
    app_installation = GithubAppInstallation.objects.create(
        workspace_integration=workspace_integration,
        installation_id=98765,
        account_login="acme-corp",
        account_type=GithubAppInstallation.AccountType.ORGANIZATION,
    )

    with patch(
        "pi_dash.app.views.integration.github.GithubClient.for_installation",
        side_effect=RuntimeError("token rejected"),
    ):
        response = session_client.post(
            reverse("github-app-refresh"),
            {"workspace_slug": workspace.slug},
            format="json",
        )

    assert response.status_code == status.HTTP_502_BAD_GATEWAY
    assert response.data == {"error": "token rejected"}
    app_installation.refresh_from_db()
    assert app_installation.last_check_error == "token rejected"
    assert app_installation.verified_at is None


def test_github_app_webhook_rejects_invalid_signature(api_client):
    with patch("pi_dash.app.views.integration.github.verify_webhook_signature", return_value=False):
        response = api_client.post(
            reverse("github-app-webhook"),
            data=json.dumps({"zen": "hello"}),
            content_type="application/json",
            HTTP_X_GITHUB_DELIVERY=str(uuid4()),
            HTTP_X_GITHUB_EVENT="ping",
            HTTP_X_HUB_SIGNATURE_256="sha256=bad",
        )

    assert response.status_code == status.HTTP_401_UNAUTHORIZED
    assert GithubWebhookDelivery.objects.count() == 0


def test_github_app_webhook_persists_ping_delivery(api_client):
    delivery_id = uuid4()

    with patch("pi_dash.app.views.integration.github.verify_webhook_signature", return_value=True):
        response = api_client.post(
            reverse("github-app-webhook"),
            data=json.dumps({"zen": "hello"}),
            content_type="application/json",
            HTTP_X_GITHUB_DELIVERY=str(delivery_id),
            HTTP_X_GITHUB_EVENT="ping",
            HTTP_X_HUB_SIGNATURE_256="sha256=valid",
        )

    assert response.status_code == status.HTTP_202_ACCEPTED
    assert response.data == {"status": GithubWebhookDelivery.Status.PROCESSED}

    delivery = GithubWebhookDelivery.objects.get(delivery_id=delivery_id)
    assert delivery.event == "ping"
    assert delivery.status == GithubWebhookDelivery.Status.PROCESSED
    assert delivery.processed_at is not None


def test_github_app_webhook_unsuspend_persists_installation_state(api_client, workspace, create_user):
    workspace_integration = _get_or_create_workspace_integration(workspace, create_user)
    app_installation = GithubAppInstallation.objects.create(
        workspace_integration=workspace_integration,
        installation_id=98765,
        account_login="acme-corp",
        account_type=GithubAppInstallation.AccountType.ORGANIZATION,
        suspended_at=timezone.now(),
        last_check_error="GitHub App installation removed or suspended",
    )
    delivery_id = uuid4()

    with (
        patch("pi_dash.app.views.integration.github.verify_webhook_signature", return_value=True),
        patch(
            "pi_dash.app.views.integration.github.GithubClient.for_installation",
            return_value=_FakeInstallationClient(repository_count=5),
        ),
    ):
        response = api_client.post(
            reverse("github-app-webhook"),
            data=json.dumps({"action": "unsuspend", "installation": {"id": 98765}}),
            content_type="application/json",
            HTTP_X_GITHUB_DELIVERY=str(delivery_id),
            HTTP_X_GITHUB_EVENT="installation",
            HTTP_X_HUB_SIGNATURE_256="sha256=valid",
        )

    assert response.status_code == status.HTTP_202_ACCEPTED
    assert response.data == {"status": GithubWebhookDelivery.Status.PROCESSED}
    app_installation.refresh_from_db()
    assert app_installation.suspended_at is None
    assert app_installation.last_check_error == ""
    assert app_installation.repository_count == 5


def test_write_only_github_app_config_sentinel_does_not_overwrite_secret(create_user):
    instance = Instance.objects.create(
        instance_name="test",
        instance_id=f"instance-{uuid4()}",
        current_version="1.0.0",
        last_checked_at=timezone.now(),
    )
    InstanceAdmin.objects.create(instance=instance, user=create_user, role=20, is_verified=True)
    configuration = InstanceConfiguration.objects.create(
        key="GITHUB_APP_PRIVATE_KEY",
        value=encrypt_data("actual-private-key"),
        category="GITHUB",
        is_encrypted=True,
    )
    client = APIClient()
    client.force_authenticate(user=create_user)

    response = client.patch(
        "/api/instances/configurations/",
        {"GITHUB_APP_PRIVATE_KEY": "set"},
        format="json",
    )

    assert response.status_code == status.HTTP_200_OK
    configuration.refresh_from_db()
    assert decrypt_data(configuration.value) == "actual-private-key"
    assert response.data[0]["value"] == "set"
    assert response.data[0]["is_write_only"] is True
