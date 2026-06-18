# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the GitHub PR ↔ work-item link endpoints.

Covers ``pidash issue attach-pr``'s server contract: attach by URL, idempotent
re-attach, one-PR-one-issue conflict, URL validation, list, detach, and the
best-effort snapshot at attach time. See
``.ai_design/github_pr_issue_link/design.md``.
"""

from unittest.mock import patch

import pytest
from rest_framework import status as http_status

from pi_dash.db.models import (
    GithubPullRequestLink,
    Issue,
    Project,
    ProjectMember,
)


pytestmark = [pytest.mark.contract, pytest.mark.django_db]


@pytest.fixture(autouse=True)
def _no_throttle(settings):
    settings.REST_FRAMEWORK = {**settings.REST_FRAMEWORK, "DEFAULT_THROTTLE_CLASSES": ()}


@pytest.fixture
def pr_project(db, workspace, create_user):
    project = Project.objects.create(
        name="PR Link Project",
        identifier="PRL",
        workspace=workspace,
        created_by=create_user,
    )
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    return project


@pytest.fixture
def issue(pr_project, create_user):
    return Issue.objects.create(
        name="change the home page button to blue",
        project=pr_project,
        workspace=pr_project.workspace,
        created_by=create_user,
    )


def _list_url(slug, project_id, issue_id):
    return f"/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/github/pull-requests/"


def _detail_url(slug, project_id, issue_id, pk):
    return f"/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/github/pull-requests/{pk}/"


def test_attach_creates_link(api_key_client, workspace, pr_project, issue):
    url = _list_url(workspace.slug, pr_project.id, issue.id)

    response = api_key_client.post(
        url, {"url": "https://github.com/Acme-Corp/Web/pull/42"}, format="json"
    )

    assert response.status_code == http_status.HTTP_201_CREATED
    assert response.data["pr_number"] == 42
    # owner/repo normalized to lowercase so the webhook can match.
    assert response.data["repo_owner"] == "acme-corp"
    assert response.data["repo_name"] == "web"
    assert response.data["url"] == "https://github.com/acme-corp/web/pull/42"

    link = GithubPullRequestLink.objects.get(pk=response.data["id"])
    assert link.issue_id == issue.id
    assert link.state == "open"
    assert link.merged is False


def test_attach_is_idempotent_for_same_issue(api_key_client, workspace, pr_project, issue):
    url = _list_url(workspace.slug, pr_project.id, issue.id)
    body = {"url": "https://github.com/acme/web/pull/7"}

    first = api_key_client.post(url, body, format="json")
    second = api_key_client.post(url, body, format="json")

    assert first.status_code == http_status.HTTP_201_CREATED
    assert second.status_code == http_status.HTTP_200_OK
    assert second.data["id"] == first.data["id"]
    assert GithubPullRequestLink.objects.filter(repo_owner="acme", repo_name="web", pr_number=7).count() == 1


def test_attach_conflict_when_pr_linked_to_other_issue(api_key_client, workspace, pr_project, issue, create_user):
    other_issue = Issue.objects.create(
        name="another issue",
        project=pr_project,
        workspace=pr_project.workspace,
        created_by=create_user,
    )
    GithubPullRequestLink.objects.create(
        project=pr_project, issue=other_issue, repo_owner="acme", repo_name="web", pr_number=9,
        url="https://github.com/acme/web/pull/9",
    )

    response = api_key_client.post(
        _list_url(workspace.slug, pr_project.id, issue.id),
        {"url": "https://github.com/acme/web/pull/9"},
        format="json",
    )

    assert response.status_code == http_status.HTTP_409_CONFLICT
    assert GithubPullRequestLink.objects.filter(repo_owner="acme", repo_name="web", pr_number=9).count() == 1


@pytest.mark.parametrize(
    "bad_url",
    [
        "",
        "not-a-url",
        "https://github.com/acme/web",  # repo, not a PR
        "https://github.com/acme/web/issues/42",  # issue, not a PR
        "https://gitlab.com/acme/web/pull/42",  # non-github host
    ],
)
def test_attach_rejects_invalid_url(api_key_client, workspace, pr_project, issue, bad_url):
    response = api_key_client.post(
        _list_url(workspace.slug, pr_project.id, issue.id), {"url": bad_url}, format="json"
    )
    assert response.status_code == http_status.HTTP_400_BAD_REQUEST
    assert GithubPullRequestLink.objects.count() == 0


def _install_app(workspace, actor, account_login="acme"):
    """Create a real WorkspaceIntegration + GithubAppInstallation so the
    best-effort snapshot path resolves an installation for ``account_login``."""
    from pi_dash.app.views.integration.github import _get_or_create_workspace_integration
    from pi_dash.db.models import GithubAppInstallation

    wi = _get_or_create_workspace_integration(workspace, actor)
    return GithubAppInstallation.objects.create(
        workspace_integration=wi, installation_id=4242, account_login=account_login
    )


class _FakePRClient:
    def __init__(self, pr):
        self._pr = pr

    def get_pull_request(self, owner, name, number):
        return self._pr


def test_attach_fills_snapshot_when_app_installed(api_key_client, workspace, pr_project, issue, create_user):
    """When the workspace has an App installation covering the account, attach
    best-effort fetches the PR to pre-fill the display snapshot."""
    _install_app(workspace, create_user, account_login="acme")
    fake_pr = {"title": "Make the button blue", "state": "open", "draft": True, "merged": False, "updated_at": "2026-06-17T09:00:00Z"}

    with patch(
        "pi_dash.utils.github_pr_links.GithubClient.for_installation",
        return_value=_FakePRClient(fake_pr),
    ):
        response = api_key_client.post(
            _list_url(workspace.slug, pr_project.id, issue.id),
            {"url": "https://github.com/Acme/Web/pull/55"},
            format="json",
        )

    assert response.status_code == http_status.HTTP_201_CREATED
    assert response.data["title"] == "Make the button blue"
    assert response.data["draft"] is True
    assert response.data["pr_updated_at"] is not None


def test_attach_survives_snapshot_failure(api_key_client, workspace, pr_project, issue, create_user):
    """A failing best-effort snapshot must not fail the attach (link still created, blank snapshot)."""
    _install_app(workspace, create_user, account_login="acme")

    with patch(
        "pi_dash.utils.github_pr_links.GithubClient.for_installation",
        side_effect=RuntimeError("boom"),
    ):
        response = api_key_client.post(
            _list_url(workspace.slug, pr_project.id, issue.id),
            {"url": "https://github.com/acme/web/pull/56"},
            format="json",
        )

    assert response.status_code == http_status.HTTP_201_CREATED
    assert response.data["title"] == ""


def test_list_returns_attached_prs(api_key_client, workspace, pr_project, issue):
    for n in (1, 2):
        GithubPullRequestLink.objects.create(
            project=pr_project, issue=issue, repo_owner="acme", repo_name="web", pr_number=n,
            url=f"https://github.com/acme/web/pull/{n}",
        )

    response = api_key_client.get(_list_url(workspace.slug, pr_project.id, issue.id))

    assert response.status_code == http_status.HTTP_200_OK
    numbers = {row["pr_number"] for row in response.data["results"]}
    assert numbers == {1, 2}


@patch("pi_dash.db.mixins.soft_delete_related_objects.delay")
def test_detach_soft_deletes_link(_delay, api_key_client, workspace, pr_project, issue):
    link = GithubPullRequestLink.objects.create(
        project=pr_project, issue=issue, repo_owner="acme", repo_name="web", pr_number=3,
        url="https://github.com/acme/web/pull/3",
    )

    response = api_key_client.delete(_detail_url(workspace.slug, pr_project.id, issue.id, link.id))

    assert response.status_code == http_status.HTTP_204_NO_CONTENT
    assert GithubPullRequestLink.objects.filter(pk=link.id).count() == 0  # soft-deleted (default manager hides it)
    # the same PR can be re-attached after detach (partial-unique on deleted_at)
    reattach = api_key_client.post(
        _list_url(workspace.slug, pr_project.id, issue.id),
        {"url": "https://github.com/acme/web/pull/3"},
        format="json",
    )
    assert reattach.status_code == http_status.HTTP_201_CREATED


def test_attach_requires_project_membership(api_client, workspace, pr_project, issue, create_user):
    """A valid API key whose user is not a member of the project is rejected."""
    from pi_dash.db.models.api import APIToken

    outsider = type(create_user).objects.create(email="outsider@example.com", username="outsider")
    token = APIToken.objects.create(user=outsider, label="outsider", token="outsider-token")
    api_client.credentials(HTTP_X_API_KEY=token.token)

    response = api_client.post(
        _list_url(workspace.slug, pr_project.id, issue.id),
        {"url": "https://github.com/acme/web/pull/11"},
        format="json",
    )

    assert response.status_code in (http_status.HTTP_403_FORBIDDEN, http_status.HTTP_404_NOT_FOUND)
    assert GithubPullRequestLink.objects.count() == 0
