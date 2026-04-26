# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""GitHub Issue Sync — Celery tasks.

See .ai_design/github_sync/design.md §6.3 (full-scan sync) and §6.5
(completion comment-back).
"""

from __future__ import annotations

import logging
from typing import Iterable

from celery import shared_task
from django.conf import settings
from django.core.exceptions import ImproperlyConfigured
from django.db import transaction
from django.utils import timezone

from pi_dash.db.models import (
    GithubCommentSync,
    GithubIssueSync,
    GithubRepositorySync,
    Issue,
    IssueComment,
    State,
)
from pi_dash.license.utils.encryption import decrypt_data
from pi_dash.utils.exception_logger import log_exception
from pi_dash.utils.github_client import (
    GithubAuthError,
    GithubClient,
    GithubNotFoundError,
    GithubPermissionError,
    parse_issue_number_from_url,
)
from pi_dash.utils.html_processor import strip_tags

logger = logging.getLogger(__name__)


# --------------------------------------------------------------------- helpers


def _is_enabled() -> bool:
    """Instance-level kill switch — see design §9 Rollout."""
    return getattr(settings, "GITHUB_SYNC_ENABLED", True)


def _resolve_token(sync: GithubRepositorySync) -> str | None:
    config = sync.workspace_integration.config or {}
    token_ciphertext = config.get("token") or ""
    if not token_ciphertext:
        return None
    return decrypt_data(token_ciphertext)


def _project_default_state(project_id) -> State | None:
    return (
        State.objects.filter(project_id=project_id, default=True).first()
        or State.objects.filter(project_id=project_id).first()
    )


def _markdown_to_html(body: str | None) -> str:
    """Render markdown to a minimal HTML representation. We don't need a full
    Markdown engine for issue bodies — paragraph-per-blank-line is enough for
    MVP and avoids pulling in a new dependency. The user-facing text is the
    upstream markdown verbatim, wrapped in <p>."""
    if not body:
        return "<p></p>"
    paragraphs = [p.strip() for p in body.split("\n\n") if p.strip()]
    if not paragraphs:
        return "<p></p>"
    return "".join(f"<p>{p.replace(chr(10), '<br/>')}</p>" for p in paragraphs)


def pi_dash_issue_url(issue: Issue) -> str:
    """Absolute deep-link to an issue in the Pi Dash UI. Used in completion
    comments posted to GitHub, where relative paths aren't clickable."""
    base = getattr(settings, "WEB_URL", None) or getattr(settings, "APP_BASE_URL", None)
    if not base:
        raise ImproperlyConfigured("WEB_URL or APP_BASE_URL must be set for GitHub completion comments")
    workspace = issue.workspace
    return f"{base.rstrip('/')}/{workspace.slug}/projects/{issue.project_id}/issues/{issue.id}"


# --------------------------------------------------------------------- upserts


def _upsert_issue(sync: GithubRepositorySync, gh_issue: dict, default_state: State | None) -> Issue:
    """Create or update the local Issue mirror. `external_source="github"`
    + `external_id=<issue number>` is the upsert key. See §6.4 for the
    `[github_<n>]` title prefix."""
    number = gh_issue["number"]
    title = gh_issue.get("title") or ""
    prefixed_name = f"[github_{number}] {title}"[:255]
    body = gh_issue.get("body") or ""
    description_html = _markdown_to_html(body)
    description_stripped = strip_tags(description_html) if description_html else ""

    defaults = {
        "name": prefixed_name,
        "description_html": description_html,
        "description_stripped": description_stripped,
        "description_json": {},
    }

    issue, created = Issue.objects.update_or_create(
        project=sync.project,
        external_source="github",
        external_id=str(number),
        defaults=defaults,
    )

    if created and default_state is not None and issue.state_id is None:
        issue.state = default_state
        issue.save(update_fields=["state"])

    # Mirror tracking row + GitHub-side timestamps (per §5).
    issue_sync, _ = GithubIssueSync.objects.update_or_create(
        repository_sync=sync,
        issue=issue,
        defaults={
            "repo_issue_id": number,
            "github_issue_id": gh_issue.get("id") or 0,
            "issue_url": gh_issue.get("html_url") or "",
            "workspace_id": sync.workspace_id,
            "project_id": sync.project_id,
            "gh_issue_created_at": gh_issue.get("created_at"),
            "gh_issue_updated_at": gh_issue.get("updated_at"),
        },
    )
    # Preserve metadata flags (completion_comment_id, etc.) — only refresh
    # the author identity field set by sync.
    user = gh_issue.get("user") or {}
    if user.get("login"):
        issue_sync.metadata["github_user_login"] = user["login"]
        issue_sync.save(update_fields=["metadata"])

    return issue


def _upsert_comment(sync: GithubRepositorySync, gh_comment: dict, parent_issue: Issue) -> None:
    """Create or update a single mirrored comment on a synced issue."""
    body = gh_comment.get("body") or ""
    rendered_html = _markdown_to_html(body)
    # See §6.4 — leading paragraph form so multi-paragraph upstream bodies
    # aren't broken by an inline prefix.
    comment_html = f"<p>[Github] </p>{rendered_html}"
    comment_stripped = f"[Github] {strip_tags(rendered_html)}".strip()

    comment, _ = IssueComment.objects.update_or_create(
        issue=parent_issue,
        external_source="github",
        external_id=str(gh_comment["id"]),
        defaults={
            "comment_html": comment_html,
            "comment_stripped": comment_stripped,
            "comment_json": {},
            "workspace_id": sync.workspace_id,
            "project_id": sync.project_id,
        },
    )
    GithubCommentSync.objects.update_or_create(
        issue_sync=GithubIssueSync.objects.get(repository_sync=sync, issue=parent_issue),
        comment=comment,
        defaults={
            "repo_comment_id": gh_comment["id"],
            "workspace_id": sync.workspace_id,
            "project_id": sync.project_id,
        },
    )


# --------------------------------------------------------------------- diff


def _reconcile_upstream_gone(sync: GithubRepositorySync, remote_issue_numbers: set[int]) -> None:
    """Flag local mirrors absent from the remote listing — see §6.3.1."""
    now = timezone.now().isoformat()
    locals_ = GithubIssueSync.objects.filter(repository_sync=sync).only("id", "repo_issue_id", "metadata")
    for ghi in locals_:
        is_present = ghi.repo_issue_id in remote_issue_numbers
        was_flagged = bool(ghi.metadata.get("upstream_gone_at"))
        if not is_present and not was_flagged:
            ghi.metadata["upstream_gone_at"] = now
            ghi.save(update_fields=["metadata"])
        elif is_present and was_flagged:
            ghi.metadata.pop("upstream_gone_at", None)
            ghi.save(update_fields=["metadata"])


# --------------------------------------------------------------------- tasks


@shared_task
def sync_all_repos() -> None:
    """Beat-driven entry point — fan out one task per enabled binding."""
    if not _is_enabled():
        return
    enabled = GithubRepositorySync.objects.filter(is_sync_enabled=True).values_list("id", flat=True)
    for sync_id in enabled:
        sync_one_repo.delay(str(sync_id))


@shared_task(bind=True, max_retries=3)
def sync_one_repo(self, sync_id: str) -> None:
    """Full-scan sync of one repo into one project. Idempotent."""
    if not _is_enabled():
        return
    try:
        sync = GithubRepositorySync.objects.select_related(
            "repository", "workspace_integration", "project"
        ).get(id=sync_id)
    except GithubRepositorySync.DoesNotExist:
        return

    token = _resolve_token(sync)
    if not token:
        sync.last_sync_error = "GitHub credential is missing or workspace integration disconnected"
        sync.save(update_fields=["last_sync_error"])
        return

    client = GithubClient(token=token)
    owner, name = sync.repository.owner, sync.repository.name
    default_state = _project_default_state(sync.project_id)
    remote_issue_numbers: set[int] = set()
    issues_by_number: dict[int, Issue] = {}

    try:
        # 1. Issues (open, non-PR).
        for gh_issue in client.list_all_open_issues(owner, name):
            if "pull_request" in gh_issue:
                continue
            issue = _upsert_issue(sync, gh_issue, default_state)
            number = gh_issue["number"]
            remote_issue_numbers.add(number)
            issues_by_number[number] = issue

        # 2. Comments — repo-wide enumeration; skip PR/closed-issue/orphan
        #    comments without a local parent (see §6.3 step 2).
        for gh_comment in client.list_all_repo_comments(owner, name):
            parent_number = parse_issue_number_from_url(gh_comment.get("issue_url") or "")
            if parent_number is None or parent_number not in remote_issue_numbers:
                continue
            parent = issues_by_number.get(parent_number)
            if parent is None:
                continue
            _upsert_comment(sync, gh_comment, parent)

        # 3. Diff for upstream-gone (deletion or closure) — §6.3.1.
        _reconcile_upstream_gone(sync, remote_issue_numbers)

        sync.last_synced_at = timezone.now()
        sync.last_sync_error = ""
        sync.save(update_fields=["last_synced_at", "last_sync_error"])

    except (GithubAuthError, GithubPermissionError, GithubNotFoundError) as e:
        # 4xx — no point retrying, surface to admin.
        sync.last_sync_error = f"{type(e).__name__}: {str(e)[:900]}"
        sync.save(update_fields=["last_sync_error"])
    except Exception as e:
        log_exception(e)
        sync.last_sync_error = str(e)[:1000]
        sync.save(update_fields=["last_sync_error"])
        raise self.retry(exc=e, countdown=60 * (2 ** self.request.retries))


@shared_task
def post_completion_comment(issue_sync_id: str) -> None:
    """Post the one-shot completion comment on the upstream GitHub issue
    when a Pi Dash issue transitions to a completed state. See §6.5."""
    if not _is_enabled():
        return
    try:
        issue_sync = GithubIssueSync.objects.select_related(
            "repository_sync__repository",
            "repository_sync__workspace_integration",
            "issue",
            "issue__workspace",
        ).get(id=issue_sync_id)
    except GithubIssueSync.DoesNotExist:
        return

    if issue_sync.metadata.get("completion_comment_id"):
        return  # already commented; idempotent short-circuit

    sync = issue_sync.repository_sync
    token = _resolve_token(sync)
    if not token:
        issue_sync.metadata["completion_comment_error"] = "credential missing or disconnected"
        issue_sync.save(update_fields=["metadata"])
        return

    body = (
        f"This issue has been completed in Pi Dash: "
        f"{pi_dash_issue_url(issue_sync.issue)}"
    )

    client = GithubClient(token=token)
    try:
        comment = client.post_issue_comment(
            owner=sync.repository.owner,
            name=sync.repository.name,
            issue_number=issue_sync.repo_issue_id,
            body=body,
        )
    except (GithubAuthError, GithubPermissionError, GithubNotFoundError) as e:
        issue_sync.metadata["completion_comment_error"] = f"{type(e).__name__}: {str(e)[:500]}"
        issue_sync.save(update_fields=["metadata"])
        return
    except Exception as e:
        log_exception(e)
        issue_sync.metadata["completion_comment_error"] = str(e)[:500]
        issue_sync.save(update_fields=["metadata"])
        return

    issue_sync.metadata["completion_comment_id"] = comment.get("id")
    issue_sync.metadata.pop("completion_comment_error", None)
    issue_sync.save(update_fields=["metadata"])
