# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the session app-API PR-link ViewSet (web overview).

The attach/dedupe logic is shared with — and exhaustively covered by — the
external-API tests; here we verify the session surface, routing, and that the
web overview can list and detach.
"""

from unittest.mock import patch

import pytest
from django.urls import reverse

from rest_framework import status

from pi_dash.db.models import GithubPullRequestLink, Issue, Project, ProjectMember


pytestmark = [pytest.mark.contract, pytest.mark.django_db]


@pytest.fixture(autouse=True)
def _no_throttle(settings):
    settings.REST_FRAMEWORK = {**settings.REST_FRAMEWORK, "DEFAULT_THROTTLE_CLASSES": ()}


@pytest.fixture
def setup(workspace, create_user):
    project = Project.objects.create(name="Web P", identifier="WBP", workspace=workspace, created_by=create_user)
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    issue = Issue.objects.create(name="i", project=project, workspace=workspace, created_by=create_user)
    return workspace, project, issue


def _list_url(workspace, project, issue):
    return reverse(
        "project-issue-github-pull-requests",
        kwargs={"slug": workspace.slug, "project_id": project.id, "issue_id": issue.id},
    )


def test_attach_and_list_via_session(session_client, setup):
    workspace, project, issue = setup
    url = _list_url(workspace, project, issue)

    created = session_client.post(url, {"url": "https://github.com/acme/web/pull/12"}, format="json")
    assert created.status_code == status.HTTP_201_CREATED

    listed = session_client.get(url)
    assert listed.status_code == status.HTTP_200_OK
    assert any(row["pr_number"] == 12 for row in listed.data)


def test_attach_invalid_url_via_session(session_client, setup):
    workspace, project, issue = setup
    response = session_client.post(_list_url(workspace, project, issue), {"url": "nope"}, format="json")
    assert response.status_code == status.HTTP_400_BAD_REQUEST


@patch("pi_dash.db.mixins.soft_delete_related_objects.delay")
def test_detach_via_session(_delay, session_client, setup):
    workspace, project, issue = setup
    link = GithubPullRequestLink.objects.create(
        project=project, issue=issue, repo_owner="acme", repo_name="web", pr_number=13,
        url="https://github.com/acme/web/pull/13",
    )
    detail = reverse(
        "project-issue-github-pull-requests",
        kwargs={"slug": workspace.slug, "project_id": project.id, "issue_id": issue.id, "pk": link.id},
    )
    response = session_client.delete(detail)
    assert response.status_code == status.HTTP_204_NO_CONTENT
    assert GithubPullRequestLink.objects.filter(pk=link.id).count() == 0
