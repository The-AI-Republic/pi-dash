"""Contract: intake-issue list ``order_by`` validation.

``IntakeIssueViewSet.list`` resolves the raw ``order_by`` parameter through
the ORM (``app/views/intake/base.py:198``). An unknown name raises
``FieldError`` and the generic 500 envelope; no user text reaches SQL. The
Rust port used to interpolate any unmapped value into its ``ORDER BY``,
which executed arbitrary SQL as any project member including guests
(PIDASHCONV-1426). These tests pin the Django behavior and would run the
injection on the old port (the sleep payload takes 2s there).
"""

import time

import pytest

from _harness.seed import GUEST
from .conftest import intake_urls

pytestmark = pytest.mark.contract

SERVER_ERROR = {"error": "Something went wrong please try again later"}


@pytest.mark.parametrize("value", ["(SELECT 1)", "id; SELECT 1"])
def test_intake_issue_list_order_by_rejects_sql_injection(make_tenant, value):
    # Reachable by guests: the list allows ADMIN/MEMBER/GUEST.
    guest = make_tenant(role=GUEST)
    urls = intake_urls(guest)
    response = guest["client"].get(urls["intake_issues"], params={"order_by": value})
    assert response.status_code == 500, response.text[:500]
    assert response.json() == SERVER_ERROR


def test_intake_issue_list_order_by_injection_does_not_execute(make_tenant):
    guest = make_tenant(role=GUEST)
    urls = intake_urls(guest)
    started = time.monotonic()
    response = guest["client"].get(
        urls["intake_issues"], params={"order_by": "(SELECT 1 FROM pg_sleep(2))"}
    )
    elapsed = time.monotonic() - started
    assert response.status_code == 500, response.text[:500]
    assert response.json() == SERVER_ERROR
    assert elapsed < 1.0, f"injected order_by took {elapsed:.2f}s"


@pytest.mark.parametrize(
    "value", ["bogus", "", "-", "name", "issue__bogus", "--created_at"]
)
def test_intake_issue_list_order_by_unknown_field_is_500(make_tenant, value):
    tenant = make_tenant()
    urls = intake_urls(tenant)
    response = tenant["client"].get(urls["intake_issues"], params={"order_by": value})
    assert response.status_code == 500, response.text[:500]
    assert response.json() == SERVER_ERROR


def test_intake_issue_list_order_by_valid_fields_still_sort(make_tenant):
    tenant = make_tenant()
    urls = intake_urls(tenant)
    seeder = tenant["seeder"]
    ws_id = tenant["workspace"]["id"]
    project_id = tenant["project"]["id"]
    state_id = seeder.create_state(ws_id, project_id, name="Order")["id"]
    intake_id = tenant["intake"]["id"]
    for hours, name in ((3, "order-a"), (2, "order-z"), (1, "order-b")):
        issue = seeder.create_issue(ws_id, project_id, state_id, name=name)
        seeder.create_intake_issue(ws_id, project_id, intake_id, issue["id"])
        tenant["_database"].execute(
            "UPDATE issues SET created_at = now() - make_interval(hours => %s) WHERE id = %s",
            (hours, issue["id"]),
        )

    def names(order_by):
        response = tenant["client"].get(
            urls["intake_issues"], params={"order_by": order_by}
        )
        assert response.status_code == 200, response.text[:500]
        return [row["issue"]["name"] for row in response.json()["results"]]

    # Default is -issue__created_at: newest first.
    response = tenant["client"].get(urls["intake_issues"])
    assert response.status_code == 200, response.text[:500]
    assert [row["issue"]["name"] for row in response.json()["results"]] == [
        "order-b",
        "order-z",
        "order-a",
    ]
    assert names("issue__created_at") == ["order-a", "order-z", "order-b"]
    assert names("issue__name") == ["order-a", "order-b", "order-z"]
