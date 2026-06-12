# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Access-control parity layer.

Tools resolve all data through these helpers, which apply *exactly* the same
membership + role checks the DRF views apply (the queryset layer composed with
the ``@allow_permission`` layer). No tool may touch unscoped ORM. Workspace is
always taken from ``deps`` (server-set), never from model arguments. See
``.ai_design/integrate_ai_agent/02-backend.md`` §5.
"""

from __future__ import annotations

from django.contrib.auth import get_user_model
from pydantic_ai import ModelRetry

from pi_dash.core import permissions as core_permissions
from pi_dash.core.querysets import member_project_issues, user_issues_queryset
from pi_dash.db.models import Issue, Project
from pi_dash.db.models.state import State

User = get_user_model()


# These subclass ModelRetry so pydantic-ai feeds the message back to the model
# (which then explains the denial / missing object to the user) instead of the
# exception propagating out of the run and failing the whole turn as "internal".
class ToolPermissionError(ModelRetry):
    """Raised when the requesting user lacks permission for a tool action."""


class ToolNotFound(ModelRetry):
    """Raised when a referenced object is outside the user's scope / missing."""


def user_for(deps):
    return User.objects.get(pk=deps.user_id)


def member_projects(deps):
    """Projects in the workspace the user is an active member of."""
    return Project.objects.filter(
        workspace__slug=deps.workspace_slug,
        project_projectmember__member_id=deps.user_id,
        project_projectmember__is_active=True,
    ).distinct()


def get_project(deps, project_id) -> Project:
    project = member_projects(deps).filter(id=project_id).first()
    if project is None:
        raise ToolNotFound(f"Project {project_id} not found or not accessible.")
    return project


def scoped_issues(deps):
    return member_project_issues(user_for(deps), deps.workspace_slug)


def my_issues(deps, scope: str = "all"):
    return user_issues_queryset(user_for(deps), deps.workspace_slug, scope=scope)


def get_issue(deps, issue_id) -> Issue:
    issue = scoped_issues(deps).filter(id=issue_id).first()
    if issue is None:
        raise ToolNotFound(f"Issue {issue_id} not found or not accessible.")
    return issue


def project_states(deps, project_id):
    get_project(deps, project_id)  # scope check
    return State.objects.filter(project_id=project_id, workspace__slug=deps.workspace_slug)


def require_project_write(deps, project_id) -> None:
    """Mirror the issue write endpoints: ADMIN/MEMBER (guests blocked)."""
    allowed = core_permissions.check_project_role(
        user_for(deps),
        deps.workspace_slug,
        project_id,
        [core_permissions.ROLE_ADMIN, core_permissions.ROLE_MEMBER],
    )
    if not allowed:
        raise ToolPermissionError(
            "You don't have permission to make changes in this project."
        )
