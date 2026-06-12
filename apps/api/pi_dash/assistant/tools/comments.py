# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Comment tools."""

from __future__ import annotations

from crum import impersonate
from django.utils.html import strip_tags
from pydantic_ai import ModelRetry, RunContext

from pi_dash.assistant.runtime.agent import assistant
from pi_dash.assistant.runtime.deps import AssistantDeps
from pi_dash.assistant.runtime.markdown import to_safe_html
from pi_dash.assistant.tools import _results, _scoping
from pi_dash.core.permissions import ROLE_GUEST
from pi_dash.db.models import IssueComment


@assistant.tool
def create_comment(ctx: RunContext[AssistantDeps], issue_id: str, body_md: str) -> dict:
    """Add a comment to an issue, attributed to you and marked 'via assistant'."""
    deps = ctx.deps
    issue = _scoping.get_issue(deps, issue_id)

    # Mirror the comment endpoint's guest rule exactly (app/views/issue/comment.py):
    # a guest may comment only when the project enables guest_view_all_features
    # or they created the issue.
    if (
        deps.workspace_role <= ROLE_GUEST
        and not issue.project.guest_view_all_features
        and issue.created_by_id != deps.user_id
    ):
        raise _scoping.ToolPermissionError(
            "Guests can only comment on issues they created."
        )

    if not body_md or not body_md.strip():
        raise ModelRetry("Comment body cannot be empty.")

    user = _scoping.user_for(deps)
    html = to_safe_html(body_md)
    with impersonate(user):
        comment = IssueComment.objects.create(
            issue=issue,
            project=issue.project,
            workspace=issue.workspace,
            actor=user,
            comment_html=html,
            comment_json={},
            comment_stripped=strip_tags(html),
            speaker_type="agent",
            speaker_label="Pi Dash AI",
        )

    _results.record_write(
        deps,
        f"Commented on issue {issue.project.identifier}-{issue.sequence_id}",
        links=[_results.issue_link(deps, issue)],
    )
    return {
        "created": True,
        "comment_id": str(comment.id),
        "issue_id": str(issue.id),
    }
