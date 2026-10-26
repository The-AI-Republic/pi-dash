"""Denied-permission and tenant-isolation coverage for the cycles domain.

The suite must fail when a permission class is deliberately removed: delete
(or comment out) the @allow_permission line on CycleViewSet.list and these
tests go red. Demonstrate that with a one-line local patch, then revert.
"""

from datetime import datetime, timedelta, timezone

import httpx
import pytest

from _harness import env
from _harness.seed import ADMIN, GUEST, MEMBER
from .conftest import create_cycle_for, cycle_urls

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


def test_guest_may_read_archived_cycles_but_not_unarchive(make_tenant):
    """PDASHOSS01-323: GUEST reads archived cycles (list + detail); the
    archive/unarchive writes stay ADMIN/MEMBER."""
    guest = make_tenant(role=GUEST)
    urls = cycle_urls(guest)
    now = datetime.now(timezone.utc)
    archived = create_cycle_for(
        guest, name="Archived for guest",
        start_date=now - timedelta(days=30), end_date=now - timedelta(days=1),
        archived_at=now,
    )
    r = guest["client"].get(urls["archived"])
    assert r.status_code == 200, r.text[:500]
    assert {row["id"] for row in r.json()} == {str(archived["id"])}
    r = guest["client"].get(urls["archived_detail"](archived["id"]))
    assert r.status_code == 200, r.text[:500]
    assert r.json()["id"] == str(archived["id"])
    r = guest["client"].delete(urls["archive"](archived["id"]))
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
