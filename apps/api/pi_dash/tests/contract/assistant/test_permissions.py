# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.core import permissions as perms

pytestmark = pytest.mark.django_db


def test_workspace_role_helpers(world):
    assert perms.workspace_role(world.admin, world.ws.id) == 20
    assert perms.workspace_role(world.member, world.ws.id) == 15
    assert perms.workspace_role(world.guest, world.ws.id) == 5
    assert perms.workspace_role(world.other_user, world.ws.id) is None

    assert perms.is_workspace_admin(world.admin, world.ws.id)
    assert not perms.is_workspace_admin(world.member, world.ws.id)

    assert perms.is_at_least_member(world.member, world.ws.id)
    assert not perms.is_at_least_member(world.guest, world.ws.id)

    assert perms.workspace_role_by_slug(world.member, world.ws.slug) == 15
    assert perms.workspace_role_by_slug(world.member, "nonexistent") is None


def test_check_project_role_admin_member_allowed_guest_denied(world):
    allowed = [perms.ROLE_ADMIN, perms.ROLE_MEMBER]
    assert perms.check_project_role(world.admin, world.ws.slug, world.proj_a.id, allowed)
    assert perms.check_project_role(world.member, world.ws.slug, world.proj_a.id, allowed)
    assert not perms.check_project_role(world.guest, world.ws.slug, world.proj_a.id, allowed)


def test_check_project_role_non_member_denied(world):
    allowed = [perms.ROLE_ADMIN, perms.ROLE_MEMBER]
    # outsider is a workspace member but NOT a project member of B
    assert not perms.check_project_role(world.outsider, world.ws.slug, world.proj_b.id, allowed)
    # cross-tenant user entirely denied
    assert not perms.check_project_role(world.other_user, world.ws.slug, world.proj_a.id, allowed)


def test_workspace_admin_bypass(world):
    # Admin is a project member of B with admin role; even asking for MEMBER-only,
    # the workspace-admin bypass grants access.
    assert perms.check_project_role(
        world.admin, world.ws.slug, world.proj_b.id, [perms.ROLE_MEMBER]
    )
