# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

from django.db import IntegrityError

from pi_dash.db.models import (
    GitCodeReviewLink,
    GitProviderAccount,
    GitRepository,
    GitRepositoryBinding,
    GithubPullRequestLink,
    Issue,
)
from pi_dash.integrations.git.adapters.base import GitProviderNotFoundError
from pi_dash.integrations.git.registry import get_adapter, parse_code_review_url
from pi_dash.integrations.git.services import account_credential, select_provider_account


class InvalidCodeReviewURL(ValueError):
    """The supplied URL is not a supported pull request or merge request URL."""


class IssueNotFound(Exception):
    """The work item does not exist in the given project/workspace."""


class CodeReviewAlreadyLinked(Exception):
    """The remote code review is already linked to a different work item."""

    def __init__(self, issue_id):
        self.issue_id = issue_id
        super().__init__(f"This code review is already linked to issue {issue_id}.")


def _github_legacy_link(parsed) -> GithubPullRequestLink | None:
    if parsed.provider != "github":
        return None
    return GithubPullRequestLink.objects.filter(
        repo_owner=parsed.namespace.lower(),
        repo_name=parsed.repo_name.lower(),
        pr_number=int(parsed.external_iid),
    ).first()


def _github_path_review_link(parsed) -> GitCodeReviewLink | None:
    if parsed.provider != "github":
        return None
    return (
        GitCodeReviewLink.objects.filter(
            provider="github",
            host_url=parsed.host_url,
            namespace=parsed.namespace.lower(),
            repo_name=parsed.repo_name.lower(),
            external_iid=parsed.external_iid,
        )
        .order_by("-created_at")
        .first()
    )


def _ensure_github_legacy_link(link: GitCodeReviewLink) -> None:
    if link.provider != "github":
        return
    number = int(link.external_iid)
    existing = GithubPullRequestLink.objects.filter(
        repo_owner=link.namespace,
        repo_name=link.repo_name,
        pr_number=number,
    ).first()
    if existing is not None:
        if existing.issue_id != link.issue_id:
            raise CodeReviewAlreadyLinked(existing.issue_id)
        existing.url = link.url
        existing.title = link.title
        existing.state = (
            GithubPullRequestLink.State.CLOSED
            if link.state in {GitCodeReviewLink.State.CLOSED, GitCodeReviewLink.State.MERGED}
            else GithubPullRequestLink.State.OPEN
        )
        existing.merged = link.merged
        existing.draft = link.draft
        existing.pr_updated_at = link.remote_updated_at
        existing.save(update_fields=["url", "title", "state", "merged", "draft", "pr_updated_at", "updated_at"])
        return
    GithubPullRequestLink.objects.create(
        project_id=link.project_id,
        issue_id=link.issue_id,
        repo_owner=link.namespace,
        repo_name=link.repo_name,
        pr_number=number,
        url=link.url,
        title=link.title,
        state=(
            GithubPullRequestLink.State.CLOSED
            if link.state in {GitCodeReviewLink.State.CLOSED, GitCodeReviewLink.State.MERGED}
            else GithubPullRequestLink.State.OPEN
        ),
        merged=link.merged,
        draft=link.draft,
        pr_updated_at=link.remote_updated_at,
    )


def detach_code_review_link(link: GitCodeReviewLink) -> None:
    legacy_link = None
    if link.provider == "github":
        try:
            number = int(link.external_iid)
        except (TypeError, ValueError):
            number = None
        if number is not None:
            legacy_link = GithubPullRequestLink.objects.filter(
                repo_owner=link.namespace,
                repo_name=link.repo_name,
                pr_number=number,
            ).first()
    link.delete()
    if legacy_link is not None and legacy_link.issue_id == link.issue_id:
        legacy_link.delete()


def _account_for_review(*, workspace_slug: str, project_id, parsed):
    binding = (
        GitRepositoryBinding.objects.filter(
            workspace__slug=workspace_slug,
            project_id=project_id,
            repository__provider=parsed.provider,
            repository__host_url=parsed.host_url,
            repository__namespace__iexact=parsed.namespace,
            repository__name__iexact=parsed.repo_name,
        )
        .select_related("provider_account", "repository")
        .first()
    )
    if binding is not None:
        return binding.provider_account, binding.repository

    issue = Issue.objects.select_related("workspace").filter(
        project_id=project_id,
        workspace__slug=workspace_slug,
    ).first()
    if issue is None:
        return None, None
    try:
        account = select_provider_account(
            workspace=issue.workspace,
            provider=parsed.provider,
            host_url=parsed.host_url,
        )
    except Exception:
        return None, None
    repo = (
        GitRepository.objects.filter(
            provider=parsed.provider,
            host_url=parsed.host_url,
            namespace__iexact=parsed.namespace,
            name__iexact=parsed.repo_name,
        )
        .first()
    )
    return account, repo


def attach_code_review(*, project_id, issue_id, workspace_slug: str, raw_url: str):
    parsed = parse_code_review_url((raw_url or "").strip())
    if parsed is None:
        raise InvalidCodeReviewURL()

    if not Issue.objects.filter(id=issue_id, project_id=project_id, workspace__slug=workspace_slug).exists():
        raise IssueNotFound()

    legacy_link = _github_legacy_link(parsed)
    if legacy_link is not None and str(legacy_link.issue_id) != str(issue_id):
        raise CodeReviewAlreadyLinked(legacy_link.issue_id)

    repo_external_id = ""
    snapshot = {}
    account, repo = _account_for_review(workspace_slug=workspace_slug, project_id=project_id, parsed=parsed)
    if repo is not None:
        repo_external_id = repo.external_id
    if account is not None:
        adapter = get_adapter(parsed.provider)
        try:
            review = adapter.get_code_review(account_credential(account), parsed)
            snapshot = {
                "external_id": review.external_id,
                "title": review.title[:500],
                "state": review.state,
                "merged": review.merged,
                "draft": review.draft,
                "remote_updated_at": review.updated_at,
                "metadata": review.metadata,
            }
            if review.metadata.get("project_id") and not repo_external_id:
                repo_external_id = str(review.metadata.get("project_id"))
        except GitProviderNotFoundError:
            raise
        except Exception:
            snapshot = {}

    lookup = {
        "provider": parsed.provider,
        "host_url": parsed.host_url,
        "external_iid": parsed.external_iid,
    }
    existing_by_path = _github_path_review_link(parsed)
    if existing_by_path is not None:
        if str(existing_by_path.issue_id) != str(issue_id):
            raise CodeReviewAlreadyLinked(existing_by_path.issue_id)
        _ensure_github_legacy_link(existing_by_path)
        return existing_by_path, False

    if repo_external_id:
        existing = GitCodeReviewLink.objects.filter(**lookup, repo_external_id=repo_external_id).first()
    else:
        existing = GitCodeReviewLink.objects.filter(
            **lookup,
            namespace=parsed.namespace.lower(),
            repo_name=parsed.repo_name.lower(),
            repo_external_id="",
        ).first()
    if existing is not None:
        if str(existing.issue_id) != str(issue_id):
            raise CodeReviewAlreadyLinked(existing.issue_id)
        _ensure_github_legacy_link(existing)
        return existing, False

    defaults = {
        "project_id": project_id,
        "issue_id": issue_id,
        "provider": parsed.provider,
        "host_url": parsed.host_url,
        "namespace": parsed.namespace.lower(),
        "repo_name": parsed.repo_name.lower(),
        "repo_external_id": repo_external_id,
        "external_iid": parsed.external_iid,
        "url": parsed.url,
        **snapshot,
    }
    try:
        link = GitCodeReviewLink.objects.create(**defaults)
        _ensure_github_legacy_link(link)
        return link, True
    except IntegrityError:
        if repo_external_id:
            existing = GitCodeReviewLink.objects.filter(**lookup, repo_external_id=repo_external_id).first()
        else:
            existing = GitCodeReviewLink.objects.filter(
                **lookup,
                namespace=parsed.namespace.lower(),
                repo_name=parsed.repo_name.lower(),
                repo_external_id="",
            ).first()
        if existing is None:
            raise
        if str(existing.issue_id) != str(issue_id):
            raise CodeReviewAlreadyLinked(existing.issue_id)
        _ensure_github_legacy_link(existing)
        return existing, False
