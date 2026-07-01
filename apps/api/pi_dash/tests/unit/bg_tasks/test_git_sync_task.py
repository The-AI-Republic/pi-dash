# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from unittest.mock import patch

import pytest
from django.utils import timezone

from pi_dash.bgtasks.git_sync_task import post_completion_comment, sync_one_binding
from pi_dash.db.models import (
    GitCommentSync,
    GitIssueSync,
    GitProviderAccount,
    GitRepository,
    GitRepositoryBinding,
    Issue,
    IssueComment,
    Project,
    State,
)
from pi_dash.integrations.git.dtos import RemoteComment, RemoteIssue


pytestmark = [pytest.mark.unit, pytest.mark.django_db]


@pytest.fixture
def sync_project(db, workspace, create_user):
    project = Project.objects.create(
        name="Generic Git Sync",
        identifier="GGS",
        workspace=workspace,
        created_by=create_user,
    )
    State.objects.create(name="Backlog", project=project, workspace=workspace, group="backlog", default=True)
    return project


@pytest.fixture
def gitlab_binding(db, workspace, sync_project, create_user):
    account = GitProviderAccount.objects.create(
        workspace=workspace,
        provider="gitlab",
        host_url="https://gitlab.com",
        auth_type="pat",
        external_account_id="u1",
        external_account_login="alice",
        display_name="alice",
        capabilities={"read_issues": True, "write_comments": True},
        credential_config={"token": "token", "host_url": "https://gitlab.com", "auth_type": "pat"},
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
        project=sync_project,
        workspace=workspace,
        repository=repo,
        provider_account=account,
        actor=create_user,
        is_sync_enabled=True,
    )


class _FakeAdapter:
    display_name = "GitLab"

    def __init__(self):
        self.posted = []

    def list_open_issues(self, _credential, _repository):
        yield RemoteIssue(
            external_id="1001",
            external_iid="7",
            title="Remote title",
            body="Remote body",
            state="opened",
            author="alice",
            web_url="https://gitlab.com/acme/web/-/issues/7",
            created_at=timezone.now(),
            updated_at=timezone.now(),
            metadata={"iid": 7},
        )

    def list_issue_comments(self, _credential, _repository, issue_iid):
        assert issue_iid == "7"
        yield RemoteComment(
            external_id="5001",
            body="Looks good",
            author="bob",
            created_at=timezone.now(),
            updated_at=timezone.now(),
            metadata={"id": 5001},
        )

    def post_issue_comment(self, _credential, _repository, issue_iid, body):
        self.posted.append((issue_iid, body))
        return RemoteComment(external_id="6001", body=body, author="bot")


def test_sync_one_binding_imports_issues_and_comments(gitlab_binding):
    fake = _FakeAdapter()

    with patch("pi_dash.bgtasks.git_sync_task.get_adapter", return_value=fake):
        sync_one_binding(str(gitlab_binding.id))

    issue = Issue.objects.get(project=gitlab_binding.project, external_source="gitlab", external_id="7")
    assert issue.name == "[gitlab_7] Remote title"
    assert "Remote body" in issue.description_html

    issue_sync = GitIssueSync.objects.get(binding=gitlab_binding, issue=issue)
    assert issue_sync.external_id == "1001"
    assert issue_sync.external_iid == "7"
    assert issue_sync.remote_state == "opened"

    comment = IssueComment.objects.get(issue=issue, external_source="gitlab", external_id="5001")
    assert "[GitLab]" in comment.comment_stripped
    assert GitCommentSync.objects.filter(issue_sync=issue_sync, comment=comment, external_id="5001").exists()

    gitlab_binding.refresh_from_db()
    assert gitlab_binding.last_sync_error == ""
    assert gitlab_binding.last_synced_at is not None


def test_sync_issue_identity_is_binding_scoped(gitlab_binding):
    fake = _FakeAdapter()

    with patch("pi_dash.bgtasks.git_sync_task.get_adapter", return_value=fake):
        sync_one_binding(str(gitlab_binding.id))

    original_issue = Issue.objects.get(project=gitlab_binding.project, external_source="gitlab", external_id="7")
    gitlab_binding.delete(soft=False)

    replacement_repo = GitRepository.objects.create(
        provider="gitlab",
        host_url="https://gitlab.com",
        external_id="100",
        namespace="acme",
        name="api",
        full_name="acme/api",
        web_url="https://gitlab.com/acme/api",
    )
    replacement_binding = GitRepositoryBinding.objects.create(
        project=gitlab_binding.project,
        workspace=gitlab_binding.workspace,
        repository=replacement_repo,
        provider_account=gitlab_binding.provider_account,
        actor=gitlab_binding.actor,
        is_sync_enabled=True,
    )

    with patch("pi_dash.bgtasks.git_sync_task.get_adapter", return_value=fake):
        sync_one_binding(str(replacement_binding.id))

    issues = list(Issue.objects.filter(project=gitlab_binding.project, external_source="gitlab", external_id="7"))
    assert len(issues) == 2
    replacement_sync = GitIssueSync.objects.get(binding=replacement_binding, external_iid="7")
    assert replacement_sync.issue_id != original_issue.id


def test_completion_comment_uses_provider_adapter(gitlab_binding, settings):
    settings.WEB_URL = "https://app.example.com"
    fake = _FakeAdapter()

    issue = Issue.objects.create(
        project=gitlab_binding.project,
        workspace=gitlab_binding.workspace,
        name="[gitlab_7] Remote title",
        external_source="gitlab",
        external_id="7",
    )
    issue_sync = GitIssueSync.objects.create(
        project=gitlab_binding.project,
        workspace=gitlab_binding.workspace,
        binding=gitlab_binding,
        issue=issue,
        provider="gitlab",
        external_id="1001",
        external_iid="7",
        web_url="https://gitlab.com/acme/web/-/issues/7",
    )

    with patch("pi_dash.bgtasks.git_sync_task.get_adapter", return_value=fake):
        post_completion_comment(str(issue_sync.id))

    issue_sync.refresh_from_db()
    assert issue_sync.metadata["completion_comment_id"] == "6001"
    assert fake.posted[0][0] == "7"
    assert "https://app.example.com/" in fake.posted[0][1]
