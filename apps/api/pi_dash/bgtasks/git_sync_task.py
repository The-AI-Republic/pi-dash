# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Provider-neutral Git issue sync tasks."""

from __future__ import annotations

import logging

from celery import shared_task
from django.conf import settings
from django.utils import timezone

from pi_dash.bgtasks.github_sync_task import _safe_render, pi_dash_issue_url
from pi_dash.db.models import GitCommentSync, GitIssueSync, GitRepositoryBinding, Issue, IssueComment, State
from pi_dash.integrations.git.adapters.base import (
    GitProviderAuthError,
    GitProviderNotFoundError,
    GitProviderPermissionError,
)
from pi_dash.integrations.git.dtos import RemoteComment, RemoteIssue, RemoteRepository
from pi_dash.integrations.git.registry import get_adapter
from pi_dash.integrations.git.services import account_credential
from pi_dash.utils.exception_logger import log_exception

logger = logging.getLogger(__name__)


def _is_enabled() -> bool:
    """Instance-level kill switch for provider-neutral Git sync."""
    return getattr(settings, "GIT_SYNC_ENABLED", getattr(settings, "GITHUB_SYNC_ENABLED", True))


def _project_default_state(project_id) -> State | None:
    return (
        State.objects.filter(project_id=project_id, default=True).first()
        or State.objects.filter(project_id=project_id).first()
    )


def _remote_repository(binding: GitRepositoryBinding) -> RemoteRepository:
    repo = binding.repository
    return RemoteRepository(
        provider=repo.provider,
        external_id=repo.external_id,
        namespace=repo.namespace,
        name=repo.name,
        full_name=repo.full_name,
        web_url=repo.web_url,
        clone_url_http=repo.clone_url_http,
        clone_url_ssh=repo.clone_url_ssh,
        default_branch=repo.default_branch,
        is_private=repo.is_private,
        metadata=repo.metadata or {},
    )


def _display_name(provider: str) -> str:
    try:
        return get_adapter(provider).display_name
    except KeyError:
        return provider.title()


def _upsert_issue(
    binding: GitRepositoryBinding,
    remote_issue: RemoteIssue,
    default_state: State | None,
) -> tuple[Issue, GitIssueSync]:
    """Create or update a local Issue mirror plus its provider-neutral sync row."""
    provider = binding.repository.provider
    issue_iid = str(remote_issue.external_iid)
    prefixed_name = f"[{provider}_{issue_iid}] {remote_issue.title}"[:255]
    description_html, description_stripped = _safe_render(remote_issue.body)

    defaults = {
        "name": prefixed_name,
        "description_html": description_html,
        "description_stripped": description_stripped,
        "description_json": {},
        "workspace_id": binding.workspace_id,
        "created_by_id": binding.actor_id,
        "updated_by_id": binding.actor_id,
    }
    issue, _created = Issue.objects.update_or_create(
        project=binding.project,
        external_source=provider,
        external_id=issue_iid,
        defaults=defaults,
    )
    Issue.objects.filter(pk=issue.pk).update(
        created_by_id=binding.actor_id,
        updated_by_id=binding.actor_id,
    )
    issue.created_by_id = binding.actor_id
    issue.updated_by_id = binding.actor_id
    _ = default_state

    issue_sync, _ = GitIssueSync.objects.update_or_create(
        binding=binding,
        external_iid=issue_iid,
        defaults={
            "issue": issue,
            "provider": provider,
            "external_id": str(remote_issue.external_id),
            "web_url": remote_issue.web_url,
            "remote_state": remote_issue.state,
            "remote_created_at": remote_issue.created_at,
            "remote_updated_at": remote_issue.updated_at,
            "workspace_id": binding.workspace_id,
            "project_id": binding.project_id,
            "created_by_id": binding.actor_id,
            "updated_by_id": binding.actor_id,
            "metadata": {
                "author": remote_issue.author,
                "remote": remote_issue.metadata,
            },
        },
    )
    return issue, issue_sync


def _upsert_comment(
    binding: GitRepositoryBinding,
    remote_comment: RemoteComment,
    parent_issue: Issue,
    parent_issue_sync: GitIssueSync,
) -> None:
    """Create or update one mirrored provider comment."""
    provider = binding.repository.provider
    provider_name = _display_name(provider)
    safe_html, safe_stripped = _safe_render(remote_comment.body)
    comment_html = f"<p>[{provider_name}] </p>{safe_html}"
    comment_stripped = f"[{provider_name}] {safe_stripped}".strip()

    comment, _ = IssueComment.objects.update_or_create(
        issue=parent_issue,
        external_source=provider,
        external_id=str(remote_comment.external_id),
        defaults={
            "comment_html": comment_html,
            "comment_stripped": comment_stripped,
            "comment_json": {},
            "workspace_id": binding.workspace_id,
            "project_id": binding.project_id,
            "actor_id": binding.actor_id,
            "created_by_id": binding.actor_id,
            "updated_by_id": binding.actor_id,
        },
    )
    IssueComment.objects.filter(pk=comment.pk).update(
        created_by_id=binding.actor_id,
        updated_by_id=binding.actor_id,
    )
    comment.created_by_id = binding.actor_id
    comment.updated_by_id = binding.actor_id

    GitCommentSync.objects.update_or_create(
        issue_sync=parent_issue_sync,
        external_id=str(remote_comment.external_id),
        defaults={
            "comment": comment,
            "provider": provider,
            "remote_created_at": remote_comment.created_at,
            "remote_updated_at": remote_comment.updated_at,
            "workspace_id": binding.workspace_id,
            "project_id": binding.project_id,
            "created_by_id": binding.actor_id,
            "updated_by_id": binding.actor_id,
            "metadata": {
                "author": remote_comment.author,
                "web_url": remote_comment.web_url,
                "remote": remote_comment.metadata,
            },
        },
    )


def _reconcile_upstream_gone(binding: GitRepositoryBinding, remote_issue_iids: set[str]) -> None:
    """Flag local mirrors absent from the remote listing."""
    now = timezone.now().isoformat()
    locals_ = GitIssueSync.objects.filter(binding=binding).only("id", "external_iid", "metadata")
    for issue_sync in locals_:
        is_present = issue_sync.external_iid in remote_issue_iids
        was_flagged = bool(issue_sync.metadata.get("upstream_gone_at"))
        if not is_present and not was_flagged:
            issue_sync.metadata["upstream_gone_at"] = now
            issue_sync.save(update_fields=["metadata"])
        elif is_present and was_flagged:
            issue_sync.metadata.pop("upstream_gone_at", None)
            issue_sync.save(update_fields=["metadata"])


@shared_task
def sync_all_bindings() -> None:
    """Beat-driven entry point: fan out one task per enabled Git binding."""
    if not _is_enabled():
        return
    enabled = GitRepositoryBinding.objects.filter(is_sync_enabled=True).values_list("id", flat=True)
    for binding_id in enabled:
        sync_one_binding.delay(str(binding_id))


@shared_task(bind=True, max_retries=3)
def sync_one_binding(self, binding_id: str) -> None:
    """Full-scan sync of one provider-neutral Git binding into one project."""
    if not _is_enabled():
        return
    try:
        binding = GitRepositoryBinding.objects.select_related(
            "repository",
            "provider_account",
            "project",
            "workspace",
            "actor",
        ).get(id=binding_id)
    except GitRepositoryBinding.DoesNotExist:
        return

    adapter = get_adapter(binding.repository.provider)
    credential = account_credential(binding.provider_account)
    repository = _remote_repository(binding)
    default_state = _project_default_state(binding.project_id)
    remote_issue_iids: set[str] = set()
    issues_by_iid: dict[str, tuple[Issue, GitIssueSync]] = {}

    try:
        for remote_issue in adapter.list_open_issues(credential, repository):
            if not remote_issue.external_iid:
                continue
            issue, issue_sync = _upsert_issue(binding, remote_issue, default_state)
            issue_iid = str(remote_issue.external_iid)
            remote_issue_iids.add(issue_iid)
            issues_by_iid[issue_iid] = (issue, issue_sync)

        for issue_iid, pair in issues_by_iid.items():
            parent_issue, parent_issue_sync = pair
            for remote_comment in adapter.list_issue_comments(credential, repository, issue_iid):
                if not remote_comment.external_id:
                    continue
                _upsert_comment(binding, remote_comment, parent_issue, parent_issue_sync)

        _reconcile_upstream_gone(binding, remote_issue_iids)

        binding.last_synced_at = timezone.now()
        binding.last_sync_error = ""
        binding.save(update_fields=["last_synced_at", "last_sync_error"])

    except (GitProviderAuthError, GitProviderPermissionError, GitProviderNotFoundError) as e:
        binding.last_sync_error = f"{type(e).__name__}: {str(e)[:900]}"
        binding.provider_account.status = "degraded"
        binding.provider_account.last_check_error = binding.last_sync_error
        binding.save(update_fields=["last_sync_error"])
        binding.provider_account.save(update_fields=["status", "last_check_error"])
    except Exception as e:
        log_exception(e)
        binding.last_sync_error = str(e)[:1000]
        binding.save(update_fields=["last_sync_error"])
        raise self.retry(exc=e, countdown=60 * (2 ** self.request.retries))


@shared_task
def post_completion_comment(issue_sync_id: str) -> None:
    """Post the one-shot completion comment on the upstream provider issue."""
    if not _is_enabled():
        return
    try:
        issue_sync = GitIssueSync.objects.select_related(
            "binding__repository",
            "binding__provider_account",
            "issue",
            "issue__workspace",
        ).get(id=issue_sync_id)
    except GitIssueSync.DoesNotExist:
        return

    if issue_sync.metadata.get("completion_comment_id"):
        return

    binding = issue_sync.binding
    body = f"This issue has been completed in Pi Dash: {pi_dash_issue_url(issue_sync.issue)}"
    adapter = get_adapter(binding.repository.provider)
    credential = account_credential(binding.provider_account)

    try:
        comment = adapter.post_issue_comment(
            credential,
            _remote_repository(binding),
            issue_sync.external_iid,
            body,
        )
    except (GitProviderAuthError, GitProviderPermissionError, GitProviderNotFoundError) as e:
        issue_sync.metadata["completion_comment_error"] = f"{type(e).__name__}: {str(e)[:500]}"
        issue_sync.save(update_fields=["metadata"])
        return
    except Exception as e:
        log_exception(e)
        issue_sync.metadata["completion_comment_error"] = str(e)[:500]
        issue_sync.save(update_fields=["metadata"])
        return

    issue_sync.metadata["completion_comment_id"] = comment.external_id
    issue_sync.metadata.pop("completion_comment_error", None)
    issue_sync.save(update_fields=["metadata"])
