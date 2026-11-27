"""Detail-route permission gate for the intake domain (DIFF-56).

Old behaviour (Django, the ``django=`` arm): ``GET``/``PATCH``
``intakes/<pk>/`` (and the ``inboxes/<pk>/`` alias) carry no
``@allow_permission`` — ``IntakeViewSet`` decorates only ``list``,
``perform_create`` and ``destroy`` (``app/views/intake/base.py:56-91``) —
so any authenticated user reads and overwrites any project's intake row.

New behaviour (Rust, the ``rust=`` arm): ``retrieve`` and
``partial_update`` are gated like ``list`` (project ADMIN or MEMBER), so
outsiders, workspace members outside the project, and project guests
answer 403 and the row is unchanged.
"""

import pytest

from _harness.diffs import expect, is_rust
from _harness.seed import ADMIN, GUEST, MEMBER
from .conftest import intake_urls

pytestmark = pytest.mark.contract

DIFF = "DIFF-56"

DENIED = {"error": "You don't have the required permissions."}


def _detail(urls, intake_id):
    return f"{urls['intakes']}{intake_id}/"


def _check_denied(get_res, patch_res, owner_client, detail, original):
    """Pin both arms: 403 + unchanged row on Rust, 200 + written row on Django."""
    assert get_res.status_code == expect(DIFF, django=200, rust=403), get_res.text[:500]
    assert patch_res.status_code == expect(DIFF, django=200, rust=403), patch_res.text[:500]
    if is_rust():
        assert get_res.json() == DENIED
        assert patch_res.json() == DENIED
    body = owner_client.get(detail).json()
    assert body["name"] == expect(DIFF, django="pwned-detail", rust=original["name"])
    assert body["view_props"] == expect(
        DIFF, django={"pwned": True}, rust=original["view_props"]
    )


def test_detail_denied_for_outsider(make_tenant):
    """Signed-up user with no workspace membership: 403, row unchanged."""
    owner = make_tenant(role=ADMIN)
    outsider = make_tenant(role=ADMIN)
    urls = intake_urls(owner)
    detail = _detail(urls, owner["intake"]["id"])
    original = owner["client"].get(detail).json()
    # The inbox alias shares the handlers, so one probe covers both routes.
    alias = f"{urls['inboxes']}{owner['intake']['id']}/"
    r_alias = outsider["client"].get(alias)
    assert r_alias.status_code == expect(DIFF, django=200, rust=403), r_alias.text[:500]
    r_get = outsider["client"].get(detail)
    r_patch = outsider["client"].patch(
        detail, json={"name": "pwned-detail", "view_props": {"pwned": True}}
    )
    _check_denied(r_get, r_patch, owner["client"], detail, original)


def test_detail_denied_for_workspace_member_outside_project(make_tenant):
    """Workspace member with no project membership: 403, row unchanged."""
    owner = make_tenant(role=ADMIN)
    peer = make_tenant(role=ADMIN)
    owner["seeder"].create_workspace_member(
        owner["workspace"]["id"], peer["user"]["id"], role=MEMBER,
    )
    urls = intake_urls(owner)
    detail = _detail(urls, owner["intake"]["id"])
    original = owner["client"].get(detail).json()
    r_get = peer["client"].get(detail)
    r_patch = peer["client"].patch(
        detail, json={"name": "pwned-detail", "view_props": {"pwned": True}}
    )
    _check_denied(r_get, r_patch, owner["client"], detail, original)


def test_detail_denied_for_project_guest(make_tenant):
    """Project guest (ADMIN/MEMBER gate): 403, row unchanged."""
    owner = make_tenant(role=ADMIN)
    guest_tenant = make_tenant(role=GUEST)
    seeder = owner["seeder"]
    guest_id = guest_tenant["user"]["id"]
    seeder.db.execute(
        "delete from project_members where member_id=%s", (guest_id,),
    )
    seeder.db.execute(
        "delete from workspace_members where member_id=%s", (guest_id,)
    )
    seeder.create_workspace_member(
        owner["workspace"]["id"], guest_id, role=GUEST,
    )
    seeder.create_project_member(
        owner["workspace"]["id"], owner["project"]["id"], guest_id,
        role=GUEST,
    )
    urls = intake_urls(owner)
    detail = _detail(urls, owner["intake"]["id"])
    original = owner["client"].get(detail).json()
    r_get = guest_tenant["client"].get(detail)
    r_patch = guest_tenant["client"].patch(
        detail, json={"name": "pwned-detail", "view_props": {"pwned": True}}
    )
    _check_denied(r_get, r_patch, owner["client"], detail, original)
