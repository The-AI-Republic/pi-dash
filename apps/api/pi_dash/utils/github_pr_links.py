# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared service for attaching GitHub pull requests to work items.

Used by both API surfaces — the external API (``pidash issue attach-pr``) and
the session app API (the web overview) — so the parse / dedupe / best-effort
snapshot logic lives in exactly one place.
"""

import logging

from django.db import IntegrityError

from pi_dash.db.models import GithubAppInstallation, GithubPullRequestLink, Issue
from pi_dash.utils.exception_logger import log_exception
from pi_dash.utils.github_client import (
    GithubClient,
    parse_github_pull_request_url,
    pr_snapshot_from_payload,
)

logger = logging.getLogger(__name__)


class InvalidPullRequestURL(ValueError):
    """The supplied string is not a valid github.com pull request URL."""


class IssueNotFound(Exception):
    """The work item does not exist in the given project/workspace."""


class PullRequestAlreadyLinked(Exception):
    """The PR is already linked to a different work item (one PR → one issue)."""

    def __init__(self, issue_id):
        self.issue_id = issue_id
        super().__init__(f"This pull request is already linked to issue {issue_id}.")


def best_effort_snapshot(workspace_slug: str, owner: str, name: str, number: int) -> dict:
    """Fetch the PR via the workspace's GitHub App installation, if one covers
    this account. Returns the display snapshot, or ``{}`` on any failure — the
    webhook keeps the link fresh regardless, so attach must never fail here."""
    installation = GithubAppInstallation.objects.filter(
        workspace_integration__workspace__slug=workspace_slug,
        account_login__iexact=owner,
    ).first()
    if installation is None:
        return {}
    try:
        pull_request = GithubClient.for_installation(installation.installation_id).get_pull_request(owner, name, number)
        return pr_snapshot_from_payload(pull_request)
    except Exception as e:  # network / token / 404 — non-fatal
        log_exception(e)
        return {}


def attach_pull_request(*, project_id, issue_id, workspace_slug: str, raw_url: str):
    """Attach (or idempotently re-attach) a PR to a work item.

    Returns ``(link, created)``. Raises :class:`InvalidPullRequestURL` for a bad
    URL, :class:`IssueNotFound` when the work item is not in the given
    project/workspace, and :class:`PullRequestAlreadyLinked` when the PR already
    belongs to a different issue.
    """
    parsed = parse_github_pull_request_url((raw_url or "").strip())
    if parsed is None:
        raise InvalidPullRequestURL()
    owner, name, number = parsed
    # GitHub owners/repos are case-insensitive; normalize so attach and the
    # webhook lookup agree.
    owner, name = owner.lower(), name.lower()

    # The permission class only proves project membership; confirm the work item
    # actually belongs to this project/workspace so a member can't attach a PR
    # to an arbitrary issue id.
    if not Issue.objects.filter(id=issue_id, project_id=project_id, workspace__slug=workspace_slug).exists():
        raise IssueNotFound()

    existing = GithubPullRequestLink.objects.filter(repo_owner=owner, repo_name=name, pr_number=number).first()
    if existing is not None:
        if str(existing.issue_id) != str(issue_id):
            raise PullRequestAlreadyLinked(existing.issue_id)
        return existing, False

    snapshot = best_effort_snapshot(workspace_slug, owner, name, number)
    try:
        link = GithubPullRequestLink.objects.create(
            project_id=project_id,
            issue_id=issue_id,
            repo_owner=owner,
            repo_name=name,
            pr_number=number,
            url=f"https://github.com/{owner}/{name}/pull/{number}",
            **snapshot,
        )
        return link, True
    except IntegrityError:
        # A concurrent attach won the partial-unique race; resolve to the row
        # that now exists instead of surfacing a 500.
        existing = GithubPullRequestLink.objects.filter(repo_owner=owner, repo_name=name, pr_number=number).first()
        if existing is None:
            raise
        if str(existing.issue_id) != str(issue_id):
            raise PullRequestAlreadyLinked(existing.issue_id)
        return existing, False
