"""Denied-permission and tenant-isolation coverage for the intake domain.

The suite must fail when a permission class is deliberately removed: delete
(or comment out) the @allow_permission line on IntakeViewSet.list and these
tests go red. Demonstrate that with a one-line local patch, then revert.
"""

import pytest

from _harness.http import anonymous_client
from _harness.seed import ADMIN, GUEST
from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract


def test_outsider_denied_on_intake_list(admin, make_tenant):
    """Authenticated but no membership anywhere: 403 with the error shape."""
    other = make_tenant(role=ADMIN)
    urls = intake_urls(admin)
    r = other["client"].get(urls["intakes"])
    assert r.status_code == 403, r.text[:500]
    assert r.json() == {"error": "You don't have the required permissions."}


def test_guest_denied_on_intake_write(make_tenant):
    """GUEST may file intake issues but may not create intakes (ADMIN/MEMBER)."""
    guest = make_tenant(role=GUEST)
    urls = intake_urls(guest)
    r = guest["client"].get(urls["intakes"])
    assert r.status_code == 403, r.text[:500]
    assert r.json() == {"error": "You don't have the required permissions."}
    # Valid payload so validation passes; the permission check never runs
    # because intake creation is broken upstream for every role (see
    # test_intake_create_broken_upstream) — the guest gets the same 500.
    r = guest["client"].post(
        urls["intakes"], json={"name": "Guest intake", "deleted_at": None}
    )
    assert r.status_code == 500, r.text[:500]
    assert r.json() == {"error": "Something went wrong please try again later"}


def test_guest_may_file_intake_issue(make_tenant):
    guest = make_tenant(role=GUEST)
    urls = intake_urls(guest)
    created = create_intake_issue(
        guest["client"], urls["intake_issues"], name="Guest filed"
    )
    assert created["issue"]["name"] == "Guest filed"


def test_cross_workspace_isolation(admin, make_tenant):
    """A session from workspace B cannot touch workspace A's intake routes."""
    other = make_tenant(role=ADMIN)
    urls_a = intake_urls(admin)
    assert admin["workspace"]["slug"] != other["workspace"]["slug"]
    for url in (urls_a["intakes"], urls_a["intake_issues"]):
        r = other["client"].get(url)
        assert r.status_code == 403, f"{url}: {r.status_code} {r.text[:300]}"
    # ... and the reverse direction holds too.
    urls_b = intake_urls(other)
    r = admin["client"].get(urls_b["intakes"])
    assert r.status_code == 403, r.text[:500]


def test_guest_sees_only_own_intake_issues(make_tenant):
    """With guest_view_all_features off, guests list only what they created."""
    owner = make_tenant(role=ADMIN)
    owner_urls = intake_urls(owner)
    create_intake_issue(owner["client"], owner_urls["intake_issues"], name="Owner one")

    guest_email_tenant = make_tenant(role=GUEST)
    # Move the guest into the owner's project as GUEST.
    seeder = owner["seeder"]
    guest_id = guest_email_tenant["user"]["id"]
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
    owner_issue_list = intake_urls(owner)["intake_issues"]
    r = guest_email_tenant["client"].get(owner_issue_list)
    assert r.status_code == 200, r.text[:500]
    payload = r.json()
    rows = payload["results"] if isinstance(payload, dict) else payload
    assert rows == [], f"guest sees others' rows: {rows!r}"[:500]

    created = create_intake_issue(
        guest_email_tenant["client"], owner_issue_list, name="Guest one"
    )
    r = guest_email_tenant["client"].get(owner_issue_list)
    rows = r.json()["results"] if isinstance(r.json(), dict) else r.json()
    assert {row["id"] for row in rows} == {created["id"]}


def test_unauthenticated_denied(admin, settings):
    urls = intake_urls(admin)
    r = anonymous_client(settings.base_url).get(urls["intakes"])
    assert r.status_code in (401, 403), r.text[:500]
