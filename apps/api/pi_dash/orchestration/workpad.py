# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Agent Workpad helpers.

The handbook tells the agent to manage exactly one ``## Agent Workpad`` comment
per issue. The cloud side provides two tiny utilities here:

1. ``get_or_create_workpad`` — look up or create that single comment.
2. ``update_workpad_body`` — edit it in place.

Authorship uses a dedicated agent/system user (``get_agent_system_user``) so
these writes stay distinguishable from human activity in the audit trail.
"""

from __future__ import annotations

from typing import Optional

from django.db import transaction

from pi_dash.db.models.issue import Issue, IssueComment
from pi_dash.db.models.user import User


WORKPAD_MARKER = "## Agent Workpad"
AGENT_USER_EMAIL = "agent@pi-dash.so"
AGENT_USER_FIRST_NAME = "Pi Dash"
AGENT_USER_LAST_NAME = "Agent"


def get_agent_system_user() -> User:
    """Return (and create on first call) the dedicated bot user used to author
    workpad comments."""
    user, created = User.objects.get_or_create(
        email=AGENT_USER_EMAIL,
        defaults={
            "first_name": AGENT_USER_FIRST_NAME,
            "last_name": AGENT_USER_LAST_NAME,
            "is_bot": True,
            "username": "pi_dash_agent",
        },
    )
    if created:
        user.set_unusable_password()
        user.save(update_fields=["password"])
    return user


def _find_workpad(issue: Issue) -> Optional[IssueComment]:
    return (
        IssueComment.objects.filter(issue=issue, comment_stripped__startswith=WORKPAD_MARKER)
        .order_by("created_at")
        .first()
    )


@transaction.atomic
def get_or_create_workpad(issue: Issue, initial_body: Optional[str] = None) -> IssueComment:
    """Return the issue's single Agent Workpad comment, creating it if absent."""
    existing = _find_workpad(issue)
    if existing is not None:
        return existing

    actor = get_agent_system_user()
    body = initial_body or f"{WORKPAD_MARKER}\n\n(initialized by agent)\n"
    comment = IssueComment.objects.create(
        issue=issue,
        project=issue.project,
        workspace=issue.workspace,
        actor=actor,
        comment_html=_to_html(body),
    )
    return comment


@transaction.atomic
def update_workpad_body(issue: Issue, markdown_body: str) -> IssueComment:
    """Edit the workpad in place; creates it if missing."""
    workpad = get_or_create_workpad(issue, initial_body=markdown_body)
    workpad.comment_html = _to_html(markdown_body)
    workpad.save(update_fields=["comment_html"])
    return workpad


def _to_html(body: str) -> str:
    """Minimal md -> html shim.

    Pi Dash comments are rich HTML. We don't run the agent's markdown through
    the editor pipeline — that would normalize whitespace and break the
    ``## Agent Workpad`` structural marker the parser keys off. Instead we
    wrap the body in a `<pre>` block so newlines, list dashes, and fenced
    code samples all render verbatim in the UI while the comment model's
    ``strip_tags`` pass produces a ``comment_stripped`` that still
    ``startswith("## Agent Workpad")``.
    """
    escaped = (
        body.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
    )
    return f"<pre>{escaped}</pre>"
