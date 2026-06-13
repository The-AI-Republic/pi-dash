# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Access-control parity (requirement 5): the assistant tools must see and do
exactly what the requesting user could by hand. These tests exercise the
admin/member/guest/non-member/other-workspace matrix against the scoping layer
that both the tools and the views share."""

import pytest

from pi_dash.assistant.tools import _scoping
from pi_dash.core.querysets import member_project_issues
from pi_dash.tests.contract.assistant.conftest import (
    ROLE_ADMIN,
    ROLE_GUEST,
    ROLE_MEMBER,
    make_deps,
)

pytestmark = pytest.mark.django_db


def _ids(qs):
    return set(str(x) for x in qs.values_list("id", flat=True))


def test_member_projects_per_role(world):
    admin_deps = make_deps(world.admin, world.ws, ROLE_ADMIN)
    member_deps = make_deps(world.member, world.ws, ROLE_MEMBER)
    guest_deps = make_deps(world.guest, world.ws, ROLE_GUEST)
    outsider_deps = make_deps(world.outsider, world.ws, ROLE_MEMBER)
    cross_deps = make_deps(world.other_user, world.ws, 0)

    assert _ids(_scoping.member_projects(admin_deps)) == {str(world.proj_a.id), str(world.proj_b.id)}
    assert _ids(_scoping.member_projects(member_deps)) == {str(world.proj_a.id)}
    assert _ids(_scoping.member_projects(guest_deps)) == {str(world.proj_a.id)}
    assert _ids(_scoping.member_projects(outsider_deps)) == set()
    assert _ids(_scoping.member_projects(cross_deps)) == set()


def test_scoped_issues_excludes_non_member_projects(world):
    member_deps = make_deps(world.member, world.ws, ROLE_MEMBER)
    ids = _ids(_scoping.scoped_issues(member_deps))
    assert str(world.issue_a.id) in ids
    assert str(world.guest_issue.id) in ids
    assert str(world.issue_b.id) not in ids  # project B: member is not a member


def test_scoped_issues_parity_with_shared_queryset(world):
    """The tool's issue scope IS the shared member_project_issues queryset that
    the views use — proving byte-for-byte parity."""
    member_deps = make_deps(world.member, world.ws, ROLE_MEMBER)
    tool_ids = _ids(_scoping.scoped_issues(member_deps))
    view_ids = _ids(member_project_issues(world.member, world.ws.slug))
    assert tool_ids == view_ids


def test_cross_tenant_issue_invisible(world):
    cross_deps = make_deps(world.other_user, world.ws, 0)
    with pytest.raises(_scoping.ToolNotFound):
        _scoping.get_issue(cross_deps, world.issue_a.id)


def test_get_project_scope_enforced(world):
    member_deps = make_deps(world.member, world.ws, ROLE_MEMBER)
    # member can reach project A
    assert _scoping.get_project(member_deps, world.proj_a.id).id == world.proj_a.id
    # but not project B
    with pytest.raises(_scoping.ToolNotFound):
        _scoping.get_project(member_deps, world.proj_b.id)


def test_require_project_write_matrix(world):
    member_deps = make_deps(world.member, world.ws, ROLE_MEMBER)
    guest_deps = make_deps(world.guest, world.ws, ROLE_GUEST)
    outsider_deps = make_deps(world.outsider, world.ws, ROLE_MEMBER)

    _scoping.require_project_write(member_deps, world.proj_a.id)  # no raise

    with pytest.raises(_scoping.ToolPermissionError):
        _scoping.require_project_write(guest_deps, world.proj_a.id)
    with pytest.raises(_scoping.ToolPermissionError):
        _scoping.require_project_write(outsider_deps, world.proj_a.id)
