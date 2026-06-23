# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from django.urls import reverse
from rest_framework import status

from pi_dash.app.views.integration.github import _get_or_create_workspace_integration
from pi_dash.db.models import GitProviderAccount, GitRepository, GitRepositoryBinding


pytestmark = [pytest.mark.contract, pytest.mark.django_db]


@pytest.fixture(autouse=True)
def _no_throttle(settings):
    settings.REST_FRAMEWORK = {**settings.REST_FRAMEWORK, "DEFAULT_THROTTLE_CLASSES": ()}


def test_github_disconnect_disables_generic_pat_bindings(session_client, workspace, project, create_user):
    wi = _get_or_create_workspace_integration(workspace, create_user)
    wi.config = {"token": "encrypted-token"}
    wi.save(update_fields=["config"])
    account = GitProviderAccount.objects.create(
        workspace=workspace,
        provider=GitProviderAccount.Provider.GITHUB,
        host_url="https://github.com",
        auth_type=GitProviderAccount.AuthType.PAT,
        external_account_id=f"pat:{wi.id}",
        display_name="GitHub PAT",
        credential_config={
            "auth_type": "pat",
            "host_url": "https://github.com",
            "token": "encrypted-token",
        },
        workspace_integration=wi,
        status=GitProviderAccount.Status.CONNECTED,
    )
    repo = GitRepository.objects.create(
        provider=GitProviderAccount.Provider.GITHUB,
        host_url="https://github.com",
        external_id="1",
        namespace="acme",
        name="web",
        full_name="acme/web",
        web_url="https://github.com/acme/web",
    )
    binding = GitRepositoryBinding.objects.create(
        project=project,
        workspace=workspace,
        repository=repo,
        provider_account=account,
        actor=create_user,
        is_sync_enabled=True,
    )

    response = session_client.post(reverse("github-integration-disconnect", kwargs={"slug": workspace.slug}))

    assert response.status_code == status.HTTP_200_OK
    binding.refresh_from_db()
    account.refresh_from_db()
    assert binding.is_sync_enabled is False
    assert binding.last_sync_error == "Workspace GitHub integration disconnected"
    assert account.status == GitProviderAccount.Status.REVOKED
