# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the workspace-permission helpers (design §5.1)."""

from __future__ import annotations

import pytest

from pi_dash.db.models import User, Workspace, WorkspaceMember
from pi_dash.runner.services.permissions import (
    ROLE_ADMIN,
    ROLE_MEMBER,
    ROLE_GUEST,
    is_at_least_member,
    is_workspace_admin,
    is_workspace_member,
    workspace_role,
)


@pytest.fixture
def other_user(db):
    from uuid import uuid4

    unique = uuid4().hex[:8]
    user = User.objects.create(
        email=f"other-{unique}@example.com",
        username=f"other_{unique}",
        first_name="O",
        last_name="Ther",
    )
    user.set_password("pw")
    user.save()
    return user


@pytest.mark.unit
def test_is_workspace_member_true_for_member(db, create_user, workspace):
    assert is_workspace_member(create_user, workspace.id) is True


@pytest.mark.unit
def test_is_workspace_member_false_for_outsider(db, other_user, workspace):
    assert is_workspace_member(other_user, workspace.id) is False


@pytest.mark.unit
def test_is_workspace_member_false_for_none_user(db, workspace):
    assert is_workspace_member(None, workspace.id) is False


@pytest.mark.unit
def test_workspace_role_returns_admin(db, create_user, workspace):
    # workspace fixture creates the member with role=20 (admin).
    assert workspace_role(create_user, workspace.id) == ROLE_ADMIN


@pytest.mark.unit
def test_workspace_role_returns_none_for_outsider(db, other_user, workspace):
    assert workspace_role(other_user, workspace.id) is None


@pytest.mark.unit
def test_is_workspace_admin_true_for_admin(db, create_user, workspace):
    assert is_workspace_admin(create_user, workspace.id) is True


@pytest.mark.unit
def test_is_workspace_admin_false_for_member(db, other_user, workspace):
    WorkspaceMember.objects.create(
        workspace=workspace, member=other_user, role=ROLE_MEMBER
    )
    assert is_workspace_admin(other_user, workspace.id) is False
    assert is_at_least_member(other_user, workspace.id) is True


@pytest.mark.unit
def test_is_at_least_member_false_for_guest(db, other_user, workspace):
    WorkspaceMember.objects.create(
        workspace=workspace, member=other_user, role=ROLE_GUEST
    )
    assert is_at_least_member(other_user, workspace.id) is False
    assert is_workspace_admin(other_user, workspace.id) is False
