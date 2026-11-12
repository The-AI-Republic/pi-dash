"""Project-guest reads on modules (PIDASHCONV-1685, drift from NEWFRONT-334).

Ported from apps/api/pi_dash/tests/contract/app/test_module_guest_reads.py
@ 52d1c6334 (Django main): guests may read a module's detail and its work
items (read-only). The work-item list applies the issue-list guest rule —
a guest only sees the issues they created unless
``project.guest_view_all_features`` is set. Every write stays
admin/member-only and non-members stay refused.

NOTE: rust-dev's apps/api predates NEWFRONT-334, so the Django leg of
this file needs main merged into rust-dev first (maintainer sync).
"""

import os
import uuid

import pytest

from .conftest import module_url, modules_url, session_headers

FORBIDDEN = {"error": "You don't have the required permissions."}


def _module_issues_url(tenant, project_id, module_id):
    return f"{modules_url(tenant, project_id)}{module_id}/issues/"


def _headers_for(seed, user):
    return session_headers(
        seed, user, user["password"], os.environ["CONTRACT_SECRET_KEY"]
    )


@pytest.fixture()
def guest_pair(seed, tenant_a, project_a):
    """A project guest (workspace role 5 + project role 5) and headers."""
    guest = seed.user()
    seed.member(tenant_a["workspace"]["id"], guest["id"], role=5)
    seed.project_member(
        project_a, tenant_a["workspace"]["id"], guest["id"], role=5
    )
    return guest, _headers_for(seed, guest)


@pytest.fixture()
def member_pair(seed, tenant_a, project_a):
    """A project member (role 15) and headers."""
    member = seed.user()
    seed.member(tenant_a["workspace"]["id"], member["id"], role=15)
    seed.project_member(
        project_a, tenant_a["workspace"]["id"], member["id"], role=15
    )
    return member, _headers_for(seed, member)


@pytest.fixture()
def outsider_pair(seed):
    """Authenticated with no membership anywhere, and headers."""
    outsider = seed.user()
    return outsider, _headers_for(seed, outsider)


@pytest.fixture()
def guest_world(seed, tenant_a, project_a, module_a, guest_pair):
    """Two bridged issues: one created by the owner, one by the guest."""
    guest, _ = guest_pair
    state_id = seed.state(project_a, tenant_a["workspace"]["id"])
    admin_issue = seed.issue(
        tenant_a["workspace"]["id"], project_a,
        name="admin issue", state_id=state_id, sequence_id=1,
    )
    guest_issue = seed.issue(
        tenant_a["workspace"]["id"], project_a,
        name="guest issue", state_id=state_id, sequence_id=2,
    )
    with seed.conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET created_by_id = %s WHERE id = %s",
            (tenant_a["user"]["id"], admin_issue),
        )
        cur.execute(
            "UPDATE issues SET created_by_id = %s WHERE id = %s",
            (guest["id"], guest_issue),
        )
    seed.module_issue(
        tenant_a["workspace"]["id"], project_a, module_a, admin_issue
    )
    seed.module_issue(
        tenant_a["workspace"]["id"], project_a, module_a, guest_issue
    )
    return {"admin_issue": admin_issue, "guest_issue": guest_issue}


def _set_guest_view_all(seed, project_id, value):
    with seed.conn.cursor() as cur:
        cur.execute(
            "UPDATE projects SET guest_view_all_features = %s WHERE id = %s",
            (value, project_id),
        )


def test_guest_module_detail_returns_200(
    api, seed, tenant_a, project_a, guest_pair, module_a
):
    _, headers = guest_pair
    name = seed.conn.execute(
        "SELECT name FROM modules WHERE id = %s", (module_a,)
    ).fetchone()[0]
    r = api.get(module_url(tenant_a, project_a, module_a), headers=headers)
    assert r.status_code == 200, r.text
    assert r.json()["name"] == name


def test_guest_module_list_still_returns_200(
    api, tenant_a, project_a, guest_pair, module_a
):
    _, headers = guest_pair
    r = api.get(modules_url(tenant_a, project_a), headers=headers)
    assert r.status_code == 200, r.text
    assert any(m["id"] == module_a for m in r.json())


def test_guest_module_issues_scoped_to_own_issues_by_default(
    api, tenant_a, project_a, guest_pair, module_a, guest_world
):
    _, headers = guest_pair
    r = api.get(
        _module_issues_url(tenant_a, project_a, module_a), headers=headers
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["count"] == 1, body
    assert body["total_count"] == 1, body
    assert body["total_results"] == 1, body
    assert [row["id"] for row in body["results"]] == [
        guest_world["guest_issue"]
    ]


def test_guest_module_issues_sees_all_with_view_all_flag(
    api, seed, tenant_a, project_a, auth_a, guest_pair, module_a, guest_world
):
    _set_guest_view_all(seed, project_a, True)
    _, headers = guest_pair
    url = _module_issues_url(tenant_a, project_a, module_a)
    r = api.get(url, headers=headers)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["count"] == 2, body
    assert {row["id"] for row in body["results"]} == {
        guest_world["admin_issue"], guest_world["guest_issue"]
    }
    # With the flag the guest gets the member's list byte for byte.
    o = api.get(url, headers=auth_a)
    assert o.status_code == 200, o.text
    assert r.content == o.content


def test_guest_module_issues_scoped_when_grouped(
    api, tenant_a, project_a, guest_pair, module_a, guest_world
):
    """The guest scoping also covers the grouped branch and its totals."""
    _, headers = guest_pair
    r = api.get(
        _module_issues_url(tenant_a, project_a, module_a), headers=headers,
        params={"group_by": "priority"},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    found = [
        row["id"]
        for bucket in body["results"].values()
        for row in bucket["results"]
    ]
    assert found == [guest_world["guest_issue"]], body
    assert body["total_count"] == 1, body
    assert body["total_results"] == 1, body
    assert sum(b["total_results"] for b in body["results"].values()) == 1


def test_member_reads_still_return_200(
    api, tenant_a, project_a, member_pair, module_a, guest_world
):
    _, headers = member_pair
    detail = api.get(
        module_url(tenant_a, project_a, module_a), headers=headers
    )
    issues = api.get(
        _module_issues_url(tenant_a, project_a, module_a), headers=headers
    )
    assert detail.status_code == 200, detail.text
    assert issues.status_code == 200, issues.text
    assert issues.json()["count"] == 2, issues.text


def test_unknown_module_still_returns_404(
    api, tenant_a, project_a, member_pair
):
    _, headers = member_pair
    r = api.get(
        module_url(tenant_a, project_a, uuid.uuid4()), headers=headers
    )
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "Module not found"}


def test_guest_create_module_refused(
    api, tenant_a, project_a, guest_pair
):
    _, headers = guest_pair
    r = api.post(
        modules_url(tenant_a, project_a), headers=headers,
        json={"name": "nope"},
    )
    assert r.status_code == 403, r.text
    assert r.json() == FORBIDDEN


def test_guest_partial_update_module_refused(
    api, tenant_a, project_a, guest_pair, module_a
):
    _, headers = guest_pair
    r = api.patch(
        module_url(tenant_a, project_a, module_a), headers=headers,
        json={"name": "nope"},
    )
    assert r.status_code == 403, r.text
    assert r.json() == FORBIDDEN


def test_guest_destroy_module_refused(
    api, tenant_a, project_a, guest_pair, module_a
):
    _, headers = guest_pair
    r = api.delete(
        module_url(tenant_a, project_a, module_a), headers=headers
    )
    assert r.status_code == 403, r.text
    assert r.json() == FORBIDDEN


def test_guest_create_module_issues_refused(
    api, tenant_a, project_a, guest_pair, module_a, guest_world
):
    _, headers = guest_pair
    r = api.post(
        _module_issues_url(tenant_a, project_a, module_a), headers=headers,
        json={"issues": [guest_world["admin_issue"]]},
    )
    assert r.status_code == 403, r.text
    assert r.json() == FORBIDDEN


def test_guest_destroy_module_issue_refused(
    api, tenant_a, project_a, guest_pair, module_a, guest_world
):
    _, headers = guest_pair
    url = (
        f"{_module_issues_url(tenant_a, project_a, module_a)}"
        f"{guest_world['guest_issue']}/"
    )
    r = api.delete(url, headers=headers)
    assert r.status_code == 403, r.text
    assert r.json() == FORBIDDEN


def test_member_partial_update_still_returns_200(
    api, tenant_a, project_a, member_pair, module_a
):
    _, headers = member_pair
    r = api.patch(
        module_url(tenant_a, project_a, module_a), headers=headers,
        json={"name": "renamed by member"},
    )
    assert r.status_code == 200, r.text
    assert r.json()["name"] == "renamed by member"


def test_outsider_detail_refused(
    api, tenant_a, project_a, outsider_pair, module_a
):
    _, headers = outsider_pair
    r = api.get(module_url(tenant_a, project_a, module_a), headers=headers)
    assert r.status_code == 403, r.text
    assert r.json() == FORBIDDEN


def test_outsider_issues_refused(
    api, tenant_a, project_a, outsider_pair, module_a, guest_world
):
    _, headers = outsider_pair
    r = api.get(
        _module_issues_url(tenant_a, project_a, module_a), headers=headers
    )
    assert r.status_code == 403, r.text
    assert r.json() == FORBIDDEN


def test_outsider_list_refused(
    api, tenant_a, project_a, outsider_pair, module_a
):
    _, headers = outsider_pair
    r = api.get(modules_url(tenant_a, project_a), headers=headers)
    assert r.status_code == 403, r.text
    assert r.json() == FORBIDDEN
