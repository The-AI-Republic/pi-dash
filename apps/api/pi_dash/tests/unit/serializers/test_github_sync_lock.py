# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Read-only lock for GitHub-synced issues and comments.

The lock predicate is "an active GithubIssueSync / GithubCommentSync row
exists" — NOT `external_source == "github"`. After unbind, the cascade
deletes those rows and edits become possible again. See .ai_design/
github_sync/design.md §6.8.
"""

import pytest
from rest_framework.exceptions import ValidationError

from pi_dash.app.serializers.issue import (
    IssueCommentSerializer,
    IssueCreateSerializer,
)
from pi_dash.db.models import (
    APIToken,
    GithubCommentSync,
    GithubIssueSync,
    GithubRepository,
    GithubRepositorySync,
    Integration,
    Issue,
    IssueComment,
    Project,
    State,
    WorkspaceIntegration,
)
from pi_dash.license.utils.encryption import encrypt_data


@pytest.fixture
def project(db, workspace):
    return Project.objects.create(name="Lock Test", identifier="LT", workspace=workspace)


@pytest.fixture
def state(db, project, workspace):
    return State.objects.create(name="Backlog", project=project, workspace=workspace, group="backlog", default=True)


@pytest.fixture
def repo_sync(db, project, workspace, create_user):
    integration, _ = Integration.objects.get_or_create(
        provider="github",
        defaults={"title": "GitHub", "verified": True, "description": {}},
    )
    api_token = APIToken.objects.create(user=create_user, workspace=workspace, user_type=1, label="t")
    wi = WorkspaceIntegration.objects.create(
        workspace=workspace,
        actor=create_user,
        integration=integration,
        api_token=api_token,
        config={"token": encrypt_data("ghp_fake")},
    )
    repo = GithubRepository.objects.create(
        project=project, workspace=workspace, name="r", owner="o", repository_id=1, url="x"
    )
    return GithubRepositorySync.objects.create(
        project=project,
        workspace=workspace,
        repository=repo,
        workspace_integration=wi,
        actor=create_user,
    )


@pytest.fixture
def synced_issue(db, project, workspace, state, repo_sync):
    issue = Issue.objects.create(
        project=project,
        workspace=workspace,
        name="[github_1] Original",
        state=state,
        external_source="github",
        external_id="1",
    )
    GithubIssueSync.objects.create(
        project=project,
        workspace=workspace,
        repository_sync=repo_sync,
        issue=issue,
        repo_issue_id=1,
        github_issue_id=1,
        issue_url="u",
        metadata={},
    )
    return issue


@pytest.fixture
def synced_comment(db, project, workspace, synced_issue, repo_sync):
    comment = IssueComment.objects.create(
        project=project,
        workspace=workspace,
        issue=synced_issue,
        comment_html="<p>[Github] </p><p>upstream</p>",
        external_source="github",
        external_id="100",
    )
    issue_sync = GithubIssueSync.objects.get(issue=synced_issue)
    GithubCommentSync.objects.create(
        project=project,
        workspace=workspace,
        issue_sync=issue_sync,
        comment=comment,
        repo_comment_id=100,
    )
    return comment


@pytest.mark.unit
@pytest.mark.django_db
class TestIssueLock:
    def test_synced_issue_rejects_name_edit(self, synced_issue):
        serializer = IssueCreateSerializer(
            instance=synced_issue,
            data={"name": "New name"},
            partial=True,
            context={"project_id": str(synced_issue.project_id), "workspace_id": str(synced_issue.workspace_id)},
        )
        with pytest.raises(ValidationError):
            serializer.is_valid(raise_exception=True)

    def test_synced_issue_rejects_description_edit(self, synced_issue):
        serializer = IssueCreateSerializer(
            instance=synced_issue,
            data={"description_html": "<p>edited</p>"},
            partial=True,
            context={"project_id": str(synced_issue.project_id), "workspace_id": str(synced_issue.workspace_id)},
        )
        with pytest.raises(ValidationError):
            serializer.is_valid(raise_exception=True)

    def test_synced_issue_allows_priority_edit(self, synced_issue):
        # Workflow fields stay editable.
        serializer = IssueCreateSerializer(
            instance=synced_issue,
            data={"priority": "high"},
            partial=True,
            context={"project_id": str(synced_issue.project_id), "workspace_id": str(synced_issue.workspace_id)},
        )
        assert serializer.is_valid(), serializer.errors

    def test_lock_releases_after_sync_row_deletion(self, synced_issue):
        # Simulate unbind cascade: delete the GithubIssueSync row.
        GithubIssueSync.objects.filter(issue=synced_issue).delete()
        serializer = IssueCreateSerializer(
            instance=synced_issue,
            data={"name": "New name"},
            partial=True,
            context={"project_id": str(synced_issue.project_id), "workspace_id": str(synced_issue.workspace_id)},
        )
        assert serializer.is_valid(), serializer.errors


@pytest.mark.unit
@pytest.mark.django_db
class TestCommentLock:
    def test_synced_comment_rejects_body_edit(self, synced_comment):
        serializer = IssueCommentSerializer(
            instance=synced_comment,
            data={"comment_html": "<p>edited</p>"},
            partial=True,
        )
        with pytest.raises(ValidationError):
            serializer.is_valid(raise_exception=True)

    def test_lock_releases_after_sync_row_deletion(self, synced_comment):
        GithubCommentSync.objects.filter(comment=synced_comment).delete()
        serializer = IssueCommentSerializer(
            instance=synced_comment,
            data={"comment_html": "<p>edited</p>"},
            partial=True,
        )
        assert serializer.is_valid(), serializer.errors

    def test_native_comment_unaffected(self, db, project, workspace, synced_issue):
        native = IssueComment.objects.create(
            project=project,
            workspace=workspace,
            issue=synced_issue,
            comment_html="<p>my note</p>",
        )
        serializer = IssueCommentSerializer(
            instance=native,
            data={"comment_html": "<p>edited</p>"},
            partial=True,
        )
        assert serializer.is_valid(), serializer.errors
