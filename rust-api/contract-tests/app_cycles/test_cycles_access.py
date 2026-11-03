"""Denied-permission and tenant-isolation coverage for the cycles domain.

The suite must fail when a permission class is deliberately removed: delete
(or comment out) the @allow_permission line on CycleViewSet.list and these
tests go red. Demonstrate that with a one-line local patch, then revert.
"""

import httpx
import pytest

from _harness import env
from _harness.seed import ADMIN, GUEST, MEMBER
from .conftest import _create_issue, create_issue_for, cycle_urls

pytestmark = pytest.mark.contract

FORBIDDEN = {"error": "You don't have the required permissions."}


def test_outsider_denied_on_cycle_list(admin, make_tenant):
    """Authenticated but no membership anywhere: 403 with the error shape."""
    other = make_tenant(role=ADMIN)
    urls = cycle_urls(admin)
    r = other["client"].get(urls["cycles"])
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN


def test_guest_may_list_but_not_create(make_tenant):
    """GUEST may list cycles but may not create them (ADMIN/MEMBER only)."""
    guest = make_tenant(role=GUEST)
    urls = cycle_urls(guest)
    r = guest["client"].get(urls["cycles"])
    assert r.status_code == 200, r.text[:500]
    r = guest["client"].post(urls["cycles"], json={"name": "Guest cycle"})
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN


def test_member_denied_on_cycle_delete(admin, make_tenant):
    """Destroy is creator-ADMIN-only; a MEMBER gets 403."""
    member_tenant = make_tenant(role=MEMBER)
    # Move the member into the admin's project as MEMBER.
    member_id = member_tenant["user"]["id"]
    conn = env.connect()
    try:
        with conn.cursor() as cur:
            cur.execute(
                "delete from project_members where member_id=%s", (member_id,)
            )
            cur.execute(
                "delete from workspace_members where member_id=%s", (member_id,)
            )
        # Tracked on the per-test seed (not the session admin seed) so
        # teardown removes them before the member's user row (FK order).
        member_tenant["seed"].member(
            admin["workspace"]["id"], member_id, role=MEMBER
        )
        member_tenant["seed"].project_member(
            admin["project"]["id"], admin["workspace"]["id"], member_id,
            role=MEMBER,
        )
    finally:
        conn.close()
    urls = cycle_urls(admin)
    r = member_tenant["client"].delete(urls["detail"](admin["cycle"]["id"]))
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN


def test_cross_workspace_isolation(admin, make_tenant):
    """A session from workspace B cannot touch workspace A's cycle routes."""
    other = make_tenant(role=ADMIN)
    urls_a = cycle_urls(admin)
    assert admin["workspace"]["slug"] != other["workspace"]["slug"]
    for url in (urls_a["cycles"], urls_a["workspace_cycles"]):
        r = other["client"].get(url)
        assert r.status_code == 403, f"{url}: {r.status_code} {r.text[:300]}"
    # ... and the reverse direction holds too.
    urls_b = cycle_urls(other)
    r = admin["client"].get(urls_b["cycles"])
    assert r.status_code == 403, r.text[:500]
    assert r.json() == FORBIDDEN


def test_unauthenticated_denied(admin):
    urls = cycle_urls(admin)
    with httpx.Client(base_url=env.base_url(), timeout=30) as anon:
        r = anon.get(urls["cycles"])
    assert r.status_code in (401, 403), r.text[:500]


def _join_project(owner, tenant, role):
    """Move ``tenant``'s user into ``owner``'s workspace + project as ``role``."""
    user_id = tenant["user"]["id"]
    conn = env.connect()
    try:
        with conn.cursor() as cur:
            cur.execute("delete from project_members where member_id=%s", (user_id,))
            cur.execute("delete from workspace_members where member_id=%s", (user_id,))
        # Tracked on the owner's seed so its teardown drops them before
        # the owner's project and workspace rows (FK order).
        owner["seed"].member(owner["workspace"]["id"], user_id, role=role)
        owner["seed"].project_member(
            owner["project"]["id"], owner["workspace"]["id"], user_id, role=role,
        )
    finally:
        conn.close()


def _set_guest_view_all(owner, value):
    conn = env.connect()
    try:
        with conn.cursor() as cur:
            cur.execute(
                "update projects set guest_view_all_features=%s where id=%s",
                (value, owner["project"]["id"]),
            )
    finally:
        conn.close()


@pytest.fixture()
def guest_cycle(make_tenant):
    """An owner tenant with two work items in its cycle (one written by a
    project GUEST) plus one work item outside the cycle."""
    owner = make_tenant(role=ADMIN)
    guest = make_tenant(role=GUEST)
    _join_project(owner, guest, GUEST)
    urls = cycle_urls(owner)
    pk = owner["cycle"]["id"]
    owner_issue = create_issue_for(owner, name="Owner item")
    owner["seq"][0] += 1
    guest_issue = _create_issue(
        owner["seed"], workspace_id=owner["workspace"]["id"],
        project_id=owner["project"]["id"], state_id=owner["state"]["id"],
        created_by_id=guest["user"]["id"], name="Guest item",
        sequence_id=owner["seq"][0],
    )
    outside = create_issue_for(owner, name="Outside item")
    r = owner["client"].post(
        urls["cycle_issues"](pk), json={"issues": [owner_issue["id"], guest_issue["id"]]}
    )
    assert r.status_code == 201, r.text[:500]
    yield {
        "owner": owner, "guest": guest, "pk": pk,
        "list": urls["cycle_issues"](pk),
        "detail": lambda issue: urls["cycle_issue_detail"](pk, issue["id"]),
        "owner_issue": owner_issue, "guest_issue": guest_issue,
        "outside": outside,
    }
    # The API-created bridges are not Seed-tracked; drop them before the
    # tenants' tracked issue rows go.
    conn = env.connect()
    try:
        with conn.cursor() as cur:
            cur.execute("delete from cycle_issues where cycle_id=%s", (pk,))
    finally:
        conn.close()


def test_guest_reads_cycle_issues_like_members(guest_cycle):
    """PDASHOSS01-310: with guest_view_all_features the guest gets the
    member's list byte for byte, grouped or not, and the detail row."""
    _set_guest_view_all(guest_cycle["owner"], True)
    owner = guest_cycle["owner"]["client"]
    guest = guest_cycle["guest"]["client"]
    for params in ({}, {"group_by": "priority"}, {"priority": "high"}):
        g = guest.get(guest_cycle["list"], params=params)
        o = owner.get(guest_cycle["list"], params=params)
        assert g.status_code == 200, f"{params}: {g.text[:500]}"
        assert g.content == o.content, params
    rows = owner.get(guest_cycle["list"]).json()["results"]
    assert sorted(row["id"] for row in rows) == sorted(
        [guest_cycle["owner_issue"]["id"], guest_cycle["guest_issue"]["id"]]
    )
    r = guest.get(guest_cycle["detail"](guest_cycle["owner_issue"]))
    assert r.status_code == 200, r.text[:500]
    assert r.json() == next(
        row for row in rows if row["id"] == guest_cycle["owner_issue"]["id"]
    )
    r = guest.get(guest_cycle["detail"](guest_cycle["outside"]))
    assert r.status_code == 404, r.text[:500]
    assert r.json() == {"error": "The required object does not exist."}


def test_guest_cycle_issues_scoped_without_view_all(guest_cycle):
    """Without guest_view_all_features the guest sees only what it created,
    the same rule as the project issues list."""
    _set_guest_view_all(guest_cycle["owner"], False)
    guest = guest_cycle["guest"]["client"]
    r = guest.get(guest_cycle["list"])
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    assert [row["id"] for row in body["results"]] == [guest_cycle["guest_issue"]["id"]]
    assert body["total_count"] == 1
    r = guest.get(guest_cycle["detail"](guest_cycle["guest_issue"]))
    assert r.status_code == 200, r.text[:500]
    r = guest.get(guest_cycle["detail"](guest_cycle["owner_issue"]))
    assert r.status_code == 404, r.text[:500]


def test_guest_cycle_issue_writes_denied(guest_cycle):
    """Every cycle-issue write stays ADMIN/MEMBER."""
    guest = guest_cycle["guest"]["client"]
    detail = guest_cycle["detail"](guest_cycle["owner_issue"])
    for method, url, kwargs in (
        ("post", guest_cycle["list"], {"json": {"issues": [guest_cycle["outside"]["id"]]}}),
        ("put", detail, {"json": {"sort_order": 1}}),
        ("patch", detail, {"json": {"sort_order": 1}}),
        ("delete", detail, {}),
    ):
        r = getattr(guest, method)(url, **kwargs)
        assert r.status_code == 403, f"{method}: {r.text[:500]}"
        assert r.json() == FORBIDDEN, method
    rows = guest_cycle["owner"]["client"].get(guest_cycle["list"]).json()["results"]
    assert len(rows) == 2
