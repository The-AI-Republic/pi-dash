# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the Issue → completion-comment-back signal.

Verifies the pre/post_save pair fires `post_completion_comment` exactly once
when an issue moves into a `completed`-group state and is mirrored from a
Git provider (generic GitIssueSync). Doesn't fire for non-synced issues,
doesn't fire on already-completed issues being re-saved, doesn't fire when
`completion_comment_id` is set. Legacy GithubIssueSync rows are ignored
(PDASHOSS01-359, backfilled by 0152).
"""

from unittest.mock import patch

import pytest

from pi_dash.db.models import (
    APIToken,
    GitIssueSync,
    GitProviderAccount,
    GitRepository,
    GitRepositoryBinding,
    GithubIssueSync,
    GithubRepository,
    GithubRepositorySync,
    Integration,
    Issue,
    Project,
    State,
    StateGroup,
    WorkspaceIntegration,
)
from pi_dash.license.utils.encryption import encrypt_data


@pytest.fixture
def project(db, workspace):
    return Project.objects.create(
        name="Signal Test",
        identifier="ST",
        workspace=workspace,
    )


@pytest.fixture
def started_state(db, project, workspace):
    return State.objects.create(
        name="In Progress",
        project=project,
        workspace=workspace,
        group=StateGroup.STARTED.value,
        default=True,
    )


@pytest.fixture
def completed_state(db, project, workspace):
    return State.objects.create(
        name="Done",
        project=project,
        workspace=workspace,
        group=StateGroup.COMPLETED.value,
    )


@pytest.fixture
def github_repo_sync(db, workspace, create_user, project):
    integration, _ = Integration.objects.get_or_create(
        provider="github",
        defaults={"title": "GitHub", "verified": True, "description": {}},
    )
    api_token = APIToken.objects.create(
        user=create_user,
        workspace=workspace,
        user_type=1,
        label="github-token",
    )
    wi = WorkspaceIntegration.objects.create(
        workspace=workspace,
        actor=create_user,
        integration=integration,
        api_token=api_token,
        config={"auth_type": "pat", "token": encrypt_data("ghp_fake")},
    )
    repo = GithubRepository.objects.create(
        project=project,
        workspace=workspace,
        name="repo",
        owner="acme",
        url="https://github.com/acme/repo",
        repository_id=42,
    )
    return GithubRepositorySync.objects.create(
        project=project,
        workspace=workspace,
        repository=repo,
        workspace_integration=wi,
        actor=create_user,
        is_sync_enabled=True,
    )


@pytest.fixture
def git_binding(db, workspace, create_user, project):
    account = GitProviderAccount.objects.create(
        workspace=workspace,
        provider="gitlab",
        host_url="https://gitlab.com",
        auth_type="pat",
        external_account_id="u1",
        external_account_login="alice",
        display_name="alice",
        capabilities={},
        credential_config={},
        status=GitProviderAccount.Status.CONNECTED,
    )
    repo = GitRepository.objects.create(
        provider="gitlab",
        host_url="https://gitlab.com",
        external_id="99",
        namespace="acme",
        name="web",
        full_name="acme/web",
        web_url="https://gitlab.com/acme/web",
    )
    return GitRepositoryBinding.objects.create(
        project=project,
        workspace=workspace,
        repository=repo,
        provider_account=account,
        actor=create_user,
        is_sync_enabled=True,
    )


@pytest.fixture
def synced_issue(db, project, workspace, started_state, git_binding):
    issue = Issue.objects.create(
        project=project,
        workspace=workspace,
        name="[github_1] Test",
        state=started_state,
        external_source="github",
        external_id="1",
    )
    GitIssueSync.objects.create(
        project=project,
        workspace=workspace,
        binding=git_binding,
        issue=issue,
        provider="gitlab",
        external_id="1001",
        external_iid="7",
    )
    return issue


@pytest.fixture
def native_issue(db, project, workspace, started_state):
    return Issue.objects.create(
        project=project,
        workspace=workspace,
        name="Native issue",
        state=started_state,
    )


@pytest.mark.unit
@pytest.mark.django_db
class TestCompletionCommentSignal:
    @patch("pi_dash.bgtasks.git_sync_task.post_completion_comment.delay")
    def test_fires_on_synced_issue_completion(self, mock_delay, synced_issue, completed_state):
        synced_issue.state = completed_state
        synced_issue.save()
        mock_delay.assert_called_once()

    @patch("pi_dash.bgtasks.git_sync_task.post_completion_comment.delay")
    def test_does_not_fire_on_native_issue_completion(self, mock_delay, native_issue, completed_state):
        native_issue.state = completed_state
        native_issue.save()
        mock_delay.assert_not_called()

    @patch("pi_dash.bgtasks.git_sync_task.post_completion_comment.delay")
    def test_does_not_fire_when_state_unchanged(self, mock_delay, synced_issue):
        # Save without changing state — no transition, no fire.
        synced_issue.name = "[github_1] Edited title"
        synced_issue.save()
        mock_delay.assert_not_called()

    @patch("pi_dash.bgtasks.git_sync_task.post_completion_comment.delay")
    def test_does_not_fire_twice_for_same_completion(
        self, mock_delay, synced_issue, completed_state, started_state
    ):
        synced_issue.state = completed_state
        synced_issue.save()
        # Reopen.
        synced_issue.state = started_state
        synced_issue.save()
        # Re-complete.
        synced_issue.state = completed_state
        synced_issue.save()
        # First completion fired once. Second completion is short-circuited
        # by the receiver because metadata.completion_comment_id was set...
        # except the task hasn't actually run (it's mocked). So the receiver
        # sees an unset metadata key and fires again. To assert
        # "fires exactly once per completion transition," we set the flag
        # manually after the first fire.
        sync = GitIssueSync.objects.get(issue=synced_issue)
        sync.metadata["completion_comment_id"] = 123
        sync.save(update_fields=["metadata"])
        mock_delay.reset_mock()

        # Reopen + re-complete after flag set.
        synced_issue.state = started_state
        synced_issue.save()
        synced_issue.state = completed_state
        synced_issue.save()
        mock_delay.assert_not_called()

    @patch("pi_dash.bgtasks.git_sync_task.post_completion_comment.delay")
    def test_legacy_github_sync_row_does_not_fire(
        self, mock_delay, db, project, workspace, started_state, completed_state,
        github_repo_sync,
    ):
        # PDASHOSS01-359: legacy-only rows are ignored (legacy poller module
        # deleted in Part 4a; generic task must not fire either).
        issue = Issue.objects.create(
            project=project,
            workspace=workspace,
            name="[github_2] Legacy only",
            state=started_state,
            external_source="github",
            external_id="2",
        )
        GithubIssueSync.objects.create(
            project=project,
            workspace=workspace,
            repository_sync=github_repo_sync,
            issue=issue,
            repo_issue_id=2,
            github_issue_id=2,
            issue_url="https://github.com/acme/repo/issues/2",
            metadata={},
        )
        issue.state = completed_state
        issue.save()
        mock_delay.assert_not_called()
