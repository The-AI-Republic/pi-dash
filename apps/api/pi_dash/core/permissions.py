# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Workspace/project membership and role helpers — single source of truth.

These were originally split between ``app/permissions/base.py`` (the
``@allow_permission`` decorator) and ``runner/services/permissions.py``
(workspace helpers). They are consolidated here so the assistant tool layer
can enforce *exactly* the same access control the views enforce, without
coupling ``pi_dash.assistant`` to ``pi_dash.runner``.

Role values come from ``pi_dash.db.models.workspace.ROLE_CHOICES``:
``Admin=20``, ``Member=15``, ``Guest=5``.
"""

from __future__ import annotations

from typing import Iterable, Optional

from pi_dash.db.models import ProjectMember, WorkspaceMember

ROLE_ADMIN = 20
ROLE_MEMBER = 15
ROLE_GUEST = 5


def is_workspace_member(user, workspace_id) -> bool:
    """True if ``user`` is a member (any role) of the given workspace."""
    if user is None or not getattr(user, "is_authenticated", False):
        return False
    return WorkspaceMember.objects.filter(
        workspace_id=workspace_id,
        workspace__platform_access_disabled_at__isnull=True,
        member=user,
        is_active=True,
    ).exists()


def workspace_role(user, workspace_id) -> Optional[int]:
    """Return the user's active role in the workspace, or ``None``."""
    if user is None or not getattr(user, "is_authenticated", False):
        return None
    return (
        WorkspaceMember.objects.filter(
            workspace_id=workspace_id,
            workspace__platform_access_disabled_at__isnull=True,
            member=user,
            is_active=True,
        )
        .values_list("role", flat=True)
        .first()
    )


def workspace_role_by_slug(user, workspace_slug) -> Optional[int]:
    """Return the user's active role in the workspace identified by slug."""
    if user is None or not getattr(user, "is_authenticated", False):
        return None
    return (
        WorkspaceMember.objects.filter(
            workspace__slug=workspace_slug,
            workspace__platform_access_disabled_at__isnull=True,
            member=user,
            is_active=True,
        )
        .values_list("role", flat=True)
        .first()
    )


def is_workspace_admin(user, workspace_id) -> bool:
    """True if ``user`` is an Admin (role >= 20) of the workspace."""
    role = workspace_role(user, workspace_id)
    return role is not None and role >= ROLE_ADMIN


def is_at_least_member(user, workspace_id) -> bool:
    """True if ``user`` is at least Member role (>=15) — not Guest."""
    role = workspace_role(user, workspace_id)
    return role is not None and role >= ROLE_MEMBER


def check_project_role(
    user,
    workspace_slug: str,
    project_id,
    allowed_roles: Iterable[int],
    *,
    allow_workspace_admin_bypass: bool = True,
) -> bool:
    """Mirror of the ``@allow_permission`` PROJECT branch (``app/permissions/base.py``).

    True when the user has an active ``ProjectMember`` row with a role in
    ``allowed_roles`` for the project, OR (when ``allow_workspace_admin_bypass``)
    the user is a project member of any role AND a workspace Admin. This is the
    exact rule the issue/comment write endpoints enforce, so the assistant's
    write tools cannot exceed what the user could do by hand.
    """
    if user is None or not getattr(user, "is_authenticated", False):
        return False

    allowed_values = [int(r) for r in allowed_roles]

    has_allowed_role = ProjectMember.objects.filter(
        member=user,
        workspace__slug=workspace_slug,
        workspace__platform_access_disabled_at__isnull=True,
        project_id=project_id,
        role__in=allowed_values,
        is_active=True,
    ).exists()
    if has_allowed_role:
        return True

    if allow_workspace_admin_bypass:
        is_project_member = ProjectMember.objects.filter(
            member=user,
            workspace__slug=workspace_slug,
            workspace__platform_access_disabled_at__isnull=True,
            project_id=project_id,
            is_active=True,
        ).exists()
        if is_project_member and WorkspaceMember.objects.filter(
            member=user,
            workspace__slug=workspace_slug,
            workspace__platform_access_disabled_at__isnull=True,
            role=ROLE_ADMIN,
            is_active=True,
        ).exists():
            return True

    return False
