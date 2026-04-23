# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Workspace-membership and role helpers for the runner app.

Extracted so runner views, pod views, validation, and orchestration can share a
single source of truth. See ``.ai_design/issue_runner/design.md`` §5.1.

Role values come from ``pi_dash.db.models.workspace.ROLE_CHOICES``:
``Admin=20``, ``Member=15``, ``Guest=5``.
"""

from __future__ import annotations

from typing import Optional

from pi_dash.db.models.workspace import WorkspaceMember

ROLE_ADMIN = 20
ROLE_MEMBER = 15
ROLE_GUEST = 5


def is_workspace_member(user, workspace_id) -> bool:
    """True if ``user`` is a member (any role) of the given workspace."""
    if user is None or not getattr(user, "is_authenticated", False):
        return False
    return WorkspaceMember.objects.filter(
        workspace_id=workspace_id, member=user
    ).exists()


def workspace_role(user, workspace_id) -> Optional[int]:
    """Return the user's role in the workspace, or ``None`` if not a member."""
    if user is None or not getattr(user, "is_authenticated", False):
        return None
    return (
        WorkspaceMember.objects.filter(workspace_id=workspace_id, member=user)
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
