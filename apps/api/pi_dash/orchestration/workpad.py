# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Agent workpad + agent system-user helpers.

Two responsibilities live here:

1. **Workpad accessors** — thin wrappers around the ``Issue.workpad`` field.
   The workpad is the coding agent's durable cross-run scratchpad (markdown
   body). It used to live in a dedicated ``## Agent Workpad`` IssueComment;
   the comment thread is now reserved for human ↔ agent conversation, so the
   workpad moved onto the Issue itself.

2. **Agent system user** — ``get_agent_system_user`` returns the dedicated bot
   account used to author agent-side comments (clarifications, blocker
   notices, "PR opened" pings). Authoring under a distinct user keeps the
   audit trail honest and lets the UI distinguish agent activity.
"""

from __future__ import annotations

from django.db import transaction

from pi_dash.db.models.issue import Issue
from pi_dash.db.models.user import User


AGENT_USERNAME = "pi_dash_agent"
AGENT_USER_EMAIL = "agent@example.com"
AGENT_USER_FIRST_NAME = "Pi Dash"
AGENT_USER_LAST_NAME = "Agent"


class AgentUserCollisionError(RuntimeError):
    """A real human account holds the reserved agent username.

    Raised instead of silently reusing the row — attributing agent writes to
    a human would contaminate the audit trail and let a human effectively
    impersonate the bot.
    """


def get_agent_system_user() -> User:
    """Return (and create on first call) the dedicated bot user used to author
    agent-side comments.

    Lookup is keyed on the unique ``username`` rather than ``email`` so the
    bot identity is not gated on the user picking (or not picking) a specific
    email address. If the reserved username is already taken by a non-bot
    account we refuse to use it — see :class:`AgentUserCollisionError`.
    """
    user, created = User.objects.get_or_create(
        username=AGENT_USERNAME,
        defaults={
            "email": AGENT_USER_EMAIL,
            "first_name": AGENT_USER_FIRST_NAME,
            "last_name": AGENT_USER_LAST_NAME,
            "is_bot": True,
        },
    )
    if created:
        user.set_unusable_password()
        user.save(update_fields=["password"])
        return user
    if not user.is_bot:
        raise AgentUserCollisionError(
            f"User {AGENT_USERNAME!r} exists but is not a bot; "
            "refusing to author agent activity under a human account."
        )
    return user


def get_workpad(issue: Issue) -> str:
    """Return the issue's workpad body (empty string if never written)."""
    return issue.workpad or ""


@transaction.atomic
def set_workpad(issue: Issue, body: str) -> Issue:
    """Overwrite the workpad body. Empty string clears it."""
    issue.workpad = body or ""
    issue.save(update_fields=["workpad", "updated_at"])
    return issue
