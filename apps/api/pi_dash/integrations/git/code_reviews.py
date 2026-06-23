# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

from django.db import IntegrityError

from pi_dash.db.models import GitCodeReviewLink, GitProviderAccount, GitRepository, GitRepositoryBinding, Issue
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
        return existing, False
