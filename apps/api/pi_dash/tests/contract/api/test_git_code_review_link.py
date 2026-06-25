# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from rest_framework import status as http_status

from pi_dash.db.models import GitCodeReviewLink, GithubPullRequestLink, Issue, Project, ProjectMember


pytestmark = [pytest.mark.contract, pytest.mark.django_db]


@pytest.fixture(autouse=True)
def _no_throttle(settings):
    settings.REST_FRAMEWORK = {**settings.REST_FRAMEWORK, "DEFAULT_THROTTLE_CLASSES": ()}


@pytest.fixture
def review_project(db, workspace, create_user):
    project = Project.objects.create(
        name="Code Review Link Project",
        identifier="CRL",
        workspace=workspace,
        created_by=create_user,
    )
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    return project


@pytest.fixture
def issue(review_project, create_user):
    return Issue.objects.create(
        name="ship the integration",
        project=review_project,
        workspace=review_project.workspace,
        created_by=create_user,
    )


def _list_url(slug, project_id, issue_id):
    return f"/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/code-reviews/"


def _detail_url(slug, project_id, issue_id, pk):
    return f"/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/code-reviews/{pk}/"


def test_attach_gitlab_merge_request(api_key_client, workspace, review_project, issue):
    response = api_key_client.post(
        _list_url(workspace.slug, review_project.id, issue.id),
        {"url": "https://gitlab.com/platform/backend/api/-/merge_requests/17"},
        format="json",
    )

    assert response.status_code == http_status.HTTP_201_CREATED
    assert response.data["provider"] == "gitlab"
    assert response.data["namespace"] == "platform/backend"
    assert response.data["repo_name"] == "api"
    assert response.data["external_iid"] == "17"

    link = GitCodeReviewLink.objects.get(pk=response.data["id"])
    assert link.issue_id == issue.id
    assert link.url == "https://gitlab.com/platform/backend/api/-/merge_requests/17"


def test_attach_github_pull_request_uses_neutral_shape(api_key_client, workspace, review_project, issue):
    response = api_key_client.post(
        _list_url(workspace.slug, review_project.id, issue.id),
        {"url": "https://github.com/Acme/Web/pull/42"},
        format="json",
    )

    assert response.status_code == http_status.HTTP_201_CREATED
    assert response.data["provider"] == "github"
    assert response.data["namespace"] == "acme"
    assert response.data["repo_name"] == "web"
    assert response.data["external_iid"] == "42"
    legacy_link = GithubPullRequestLink.objects.get(repo_owner="acme", repo_name="web", pr_number=42)
    assert legacy_link.issue_id == issue.id


def test_attach_is_idempotent_for_same_issue(api_key_client, workspace, review_project, issue):
    url = _list_url(workspace.slug, review_project.id, issue.id)
    body = {"url": "https://gitlab.com/acme/web/-/merge_requests/7"}

    first = api_key_client.post(url, body, format="json")
    second = api_key_client.post(url, body, format="json")

    assert first.status_code == http_status.HTTP_201_CREATED
    assert second.status_code == http_status.HTTP_200_OK
    assert second.data["id"] == first.data["id"]
    assert GitCodeReviewLink.objects.filter(provider="gitlab", namespace="acme", repo_name="web").count() == 1


def test_attach_conflict_when_review_linked_to_other_issue(api_key_client, workspace, review_project, issue, create_user):
    other_issue = Issue.objects.create(
        name="another issue",
        project=review_project,
        workspace=review_project.workspace,
        created_by=create_user,
    )
    GitCodeReviewLink.objects.create(
        project=review_project,
        workspace=review_project.workspace,
        issue=other_issue,
        provider="gitlab",
        host_url="https://gitlab.com",
        namespace="acme",
        repo_name="web",
        external_iid="9",
        url="https://gitlab.com/acme/web/-/merge_requests/9",
    )

    response = api_key_client.post(
        _list_url(workspace.slug, review_project.id, issue.id),
        {"url": "https://gitlab.com/acme/web/-/merge_requests/9"},
        format="json",
    )

    assert response.status_code == http_status.HTTP_409_CONFLICT


def test_attach_github_conflicts_when_legacy_pr_linked_to_other_issue(
    api_key_client,
    workspace,
    review_project,
    issue,
    create_user,
):
    other_issue = Issue.objects.create(
        name="another issue",
        project=review_project,
        workspace=review_project.workspace,
        created_by=create_user,
    )
    GithubPullRequestLink.objects.create(
        project=review_project,
        issue=other_issue,
        repo_owner="acme",
        repo_name="web",
        pr_number=10,
        url="https://github.com/acme/web/pull/10",
    )

    response = api_key_client.post(
        _list_url(workspace.slug, review_project.id, issue.id),
        {"url": "https://github.com/acme/web/pull/10"},
        format="json",
    )

    assert response.status_code == http_status.HTTP_409_CONFLICT
    assert GitCodeReviewLink.objects.filter(provider="github", namespace="acme", repo_name="web").count() == 0


def test_list_and_detach(api_key_client, workspace, review_project, issue):
    link = GitCodeReviewLink.objects.create(
        project=review_project,
        workspace=review_project.workspace,
        issue=issue,
        provider="gitlab",
        host_url="https://gitlab.com",
        namespace="acme",
        repo_name="web",
        external_iid="3",
        url="https://gitlab.com/acme/web/-/merge_requests/3",
    )

    list_response = api_key_client.get(_list_url(workspace.slug, review_project.id, issue.id))
    assert list_response.status_code == http_status.HTTP_200_OK
    assert list_response.data["results"][0]["id"] == link.id

    delete_response = api_key_client.delete(_detail_url(workspace.slug, review_project.id, issue.id, link.id))
    assert delete_response.status_code == http_status.HTTP_204_NO_CONTENT


def test_detach_github_code_review_soft_deletes_legacy_pr_link(api_key_client, workspace, review_project, issue):
    link = GitCodeReviewLink.objects.create(
        project=review_project,
        issue=issue,
        provider="github",
        host_url="https://github.com",
        namespace="acme",
        repo_name="web",
        external_iid="12",
        url="https://github.com/acme/web/pull/12",
    )
    legacy_link = GithubPullRequestLink.objects.create(
        project=review_project,
        issue=issue,
        repo_owner="acme",
        repo_name="web",
        pr_number=12,
        url="https://github.com/acme/web/pull/12",
    )

    delete_response = api_key_client.delete(_detail_url(workspace.slug, review_project.id, issue.id, link.id))

    assert delete_response.status_code == http_status.HTTP_204_NO_CONTENT
    assert GitCodeReviewLink.objects.filter(pk=link.id).count() == 0
    assert GithubPullRequestLink.objects.filter(pk=legacy_link.id).count() == 0
