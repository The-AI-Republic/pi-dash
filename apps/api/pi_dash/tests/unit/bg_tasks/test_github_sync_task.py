# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Unit tests for GitHub Issue Sync — reconcile + completion-comment paths.

Covers:
  - reconcile_upstream_gone idempotency (flag set once, cleared on reappear)
  - post_completion_comment idempotency (no second post once comment_id set)
  - post_completion_comment 404 handling (writes error, doesn't crash)
  - upsert_issue produces the [github_<n>] title prefix
  - upsert_comment produces the [Github] body prefix
"""

from unittest.mock import patch, MagicMock

import pytest

from pi_dash.bgtasks.github_sync_task import (
    _reconcile_upstream_gone,
    _upsert_comment,
    _upsert_issue,
    post_completion_comment,
)
from pi_dash.db.models import (
    APIToken,
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
from pi_dash.utils.github_client import GithubNotFoundError


# --------------------------------------------------------------------- fixtures


@pytest.fixture
def project(db, workspace, create_user):
    proj = Project.objects.create(
        name="Sync Test Project",
        identifier="STP",
        workspace=workspace,
    )
    State.objects.create(
        name="Backlog",
        project=proj,
        workspace=workspace,
        group="backlog",
        default=True,
    )
    return proj


@pytest.fixture
def github_integration(db):
    integration, _ = Integration.objects.get_or_create(
        provider="github",
        defaults={"title": "GitHub", "verified": True, "description": {}},
    )
    return integration


@pytest.fixture
def github_repo_sync(db, workspace, create_user, project, github_integration):
    api_token = APIToken.objects.create(
        user=create_user,
        workspace=workspace,
        user_type=1,
        label="github-int-token",
    )
    wi = WorkspaceIntegration.objects.create(
        workspace=workspace,
        actor=create_user,
        integration=github_integration,
        api_token=api_token,
        config={"auth_type": "pat", "token": encrypt_data("ghp_fake")},
    )
    repo = GithubRepository.objects.create(
        project=project,
        workspace=workspace,
        name="repo",
        owner="acme",
        url="https://github.com/acme/repo",
        repository_id=12345,
    )
    return GithubRepositorySync.objects.create(
        project=project,
        workspace=workspace,
        repository=repo,
        workspace_integration=wi,
        actor=create_user,
        is_sync_enabled=True,
    )


# --------------------------------------------------------------------- tests


@pytest.mark.unit
class TestUpsertIssue:
    @pytest.mark.django_db
    def test_first_import_prefixes_title(self, github_repo_sync):
        gh_issue = {
            "id": 999,
            "number": 7,
            "title": "Clicking the foo deletes the bar",
            "body": "## Steps\n\n1. click foo",
            "user": {"login": "octocat"},
            "html_url": "https://github.com/acme/repo/issues/7",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-02T00:00:00Z",
        }
        issue = _upsert_issue(github_repo_sync, gh_issue, default_state=None)
        issue.refresh_from_db()
        assert issue.name.startswith("[github_7] ")
        assert issue.external_source == "github"
        assert issue.external_id == "7"

        sync = GithubIssueSync.objects.get(repository_sync=github_repo_sync, issue=issue)
        assert sync.repo_issue_id == 7
        assert sync.metadata.get("github_user_login") == "octocat"

    @pytest.mark.django_db
    def test_second_pass_overwrites_synced_fields(self, github_repo_sync):
        gh = {
            "id": 1,
            "number": 1,
            "title": "Original",
            "body": "first body",
            "user": {"login": "a"},
            "html_url": "u",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-02T00:00:00Z",
        }
        _upsert_issue(github_repo_sync, gh, default_state=None)

        gh["title"] = "Edited Upstream"
        gh["body"] = "second body"
        _upsert_issue(github_repo_sync, gh, default_state=None)

        issue = Issue.objects.get(external_source="github", external_id="1")
        assert issue.name == "[github_1] Edited Upstream"
        assert "second body" in issue.description_html


@pytest.mark.unit
class TestUpsertComment:
    @pytest.mark.django_db
    def test_prefixes_comment_body(self, github_repo_sync):
        parent = _upsert_issue(
            github_repo_sync,
            {
                "id": 1,
                "number": 1,
                "title": "Parent",
                "body": "x",
                "user": {"login": "a"},
                "html_url": "u",
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-02T00:00:00Z",
            },
            default_state=None,
        )
        gh_comment = {
            "id": 100,
            "body": "Looks broken to me.",
            "user": {"login": "octocat"},
            "issue_url": "https://api.github.com/repos/acme/repo/issues/1",
        }
        _upsert_comment(github_repo_sync, gh_comment, parent)

        comment = IssueComment.objects.get(issue=parent, external_id="100")
        assert comment.external_source == "github"
        assert "[Github]" in comment.comment_html
        assert comment.comment_stripped.startswith("[Github] ")


@pytest.mark.unit
class TestReconcileUpstreamGone:
    @pytest.mark.django_db
    def test_flag_set_once_when_absent(self, github_repo_sync):
        issue_sync = _make_issue_sync(github_repo_sync, number=42)
        _reconcile_upstream_gone(github_repo_sync, remote_issue_numbers=set())
        issue_sync.refresh_from_db()
        assert "upstream_gone_at" in issue_sync.metadata
        first_value = issue_sync.metadata["upstream_gone_at"]

        # Re-run with the same absent set; flag stays the same (not re-stamped).
        _reconcile_upstream_gone(github_repo_sync, remote_issue_numbers=set())
        issue_sync.refresh_from_db()
        assert issue_sync.metadata["upstream_gone_at"] == first_value

    @pytest.mark.django_db
    def test_flag_clears_on_reappear(self, github_repo_sync):
        issue_sync = _make_issue_sync(github_repo_sync, number=42)
        issue_sync.metadata = {"upstream_gone_at": "2026-01-01T00:00:00+00:00"}
        issue_sync.save(update_fields=["metadata"])

        _reconcile_upstream_gone(github_repo_sync, remote_issue_numbers={42})
        issue_sync.refresh_from_db()
        assert "upstream_gone_at" not in issue_sync.metadata

    @pytest.mark.django_db
    def test_no_change_when_present(self, github_repo_sync):
        issue_sync = _make_issue_sync(github_repo_sync, number=42)
        original_metadata = dict(issue_sync.metadata)
        _reconcile_upstream_gone(github_repo_sync, remote_issue_numbers={42})
        issue_sync.refresh_from_db()
        assert issue_sync.metadata == original_metadata


@pytest.mark.unit
class TestPostCompletionComment:
    @pytest.mark.django_db
    @patch("pi_dash.bgtasks.github_sync_task.GithubClient")
    def test_idempotent_short_circuit(self, mock_client_cls, github_repo_sync, settings):
        settings.WEB_URL = "https://app.example.com"
        issue_sync = _make_issue_sync(github_repo_sync, number=7)
        issue_sync.metadata["completion_comment_id"] = 12345
        issue_sync.save(update_fields=["metadata"])

        post_completion_comment(str(issue_sync.id))
        # Client never instantiated because we short-circuit before token use.
        mock_client_cls.assert_not_called()

    @pytest.mark.django_db
    @patch("pi_dash.bgtasks.github_sync_task.GithubClient")
    def test_posts_and_stores_comment_id(self, mock_client_cls, github_repo_sync, settings):
        settings.WEB_URL = "https://app.example.com"
        issue_sync = _make_issue_sync(github_repo_sync, number=7)

        mock_client = MagicMock()
        mock_client.post_issue_comment.return_value = {"id": 98765}
        mock_client_cls.return_value = mock_client

        post_completion_comment(str(issue_sync.id))

        issue_sync.refresh_from_db()
        assert issue_sync.metadata["completion_comment_id"] == 98765
        assert "completion_comment_error" not in issue_sync.metadata
        mock_client.post_issue_comment.assert_called_once()
        kwargs = mock_client.post_issue_comment.call_args.kwargs
        assert kwargs["owner"] == "acme"
        assert kwargs["name"] == "repo"
        assert kwargs["issue_number"] == 7
        assert "completed in Pi Dash" in kwargs["body"]

    @pytest.mark.django_db
    @patch("pi_dash.bgtasks.github_sync_task.GithubClient")
    def test_404_writes_error_metadata(self, mock_client_cls, github_repo_sync, settings):
        settings.WEB_URL = "https://app.example.com"
        issue_sync = _make_issue_sync(github_repo_sync, number=7)

        mock_client = MagicMock()
        mock_client.post_issue_comment.side_effect = GithubNotFoundError("issue gone")
        mock_client_cls.return_value = mock_client

        # Should not raise.
        post_completion_comment(str(issue_sync.id))

        issue_sync.refresh_from_db()
        assert "completion_comment_id" not in issue_sync.metadata
        assert "completion_comment_error" in issue_sync.metadata


# --------------------------------------------------------------------- helpers


def _make_issue_sync(repo_sync: GithubRepositorySync, *, number: int) -> GithubIssueSync:
    """Build a minimal GithubIssueSync + parent Issue for tests."""
    issue = Issue.objects.create(
        project=repo_sync.project,
        workspace=repo_sync.workspace,
        name=f"[github_{number}] Test",
        external_source="github",
        external_id=str(number),
    )
    return GithubIssueSync.objects.create(
        project=repo_sync.project,
        workspace=repo_sync.workspace,
        repository_sync=repo_sync,
        issue=issue,
        repo_issue_id=number,
        github_issue_id=number,
        issue_url=f"https://github.com/acme/repo/issues/{number}",
        metadata={},
    )
