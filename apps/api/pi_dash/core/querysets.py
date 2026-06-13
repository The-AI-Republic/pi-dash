# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared issue querysets used by both the views and the assistant tools.

Keeping the "my issues" definition in one place guarantees the assistant's
``list_my_issues`` tool returns exactly what the workspace profile-issues
endpoint returns. See ``.ai_design/integrate_ai_agent/02-backend.md`` §5.
"""

from __future__ import annotations

from django.db.models import Q

from pi_dash.db.models import Issue


def member_project_issues(user, workspace_slug):
    """Issues in ``workspace_slug`` whose project the user is an active member of.

    This is the queryset layer (``IssueViewSet.get_queryset``) composed with the
    membership layer (the ``@allow_permission`` project-member check) that the
    issue list endpoints apply together.
    """
    return Issue.issue_objects.filter(
        workspace__slug=workspace_slug,
        project__project_projectmember__member=user,
        project__project_projectmember__is_active=True,
    ).distinct()


def user_issues_queryset(user, workspace_slug, *, scope: str = "all"):
    """Issues the user is involved in, scoped to projects they belong to.

    ``scope``: ``all`` (assigned OR created OR subscribed), ``assigned``,
    ``created``. Mirrors ``WorkspaceUserProfileIssuesEndpoint`` while staying
    inside the member-projects scope (defence in depth).
    """
    base = member_project_issues(user, workspace_slug)
    user_id = user.id
    if scope == "assigned":
        involvement = Q(assignees__id=user_id)
    elif scope == "created":
        involvement = Q(created_by_id=user_id)
    else:
        involvement = (
            Q(assignees__id=user_id)
            | Q(created_by_id=user_id)
            | Q(issue_subscribers__subscriber_id=user_id)
        )
    return base.filter(involvement).distinct()
