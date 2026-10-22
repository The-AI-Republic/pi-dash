"""Contract: public issue list ``order_by`` validation.

``ProjectIssuesPublicEndpoint.get`` passes the raw ``order_by`` parameter to
Django's ``order_issue_queryset`` (``space/views/issue.py:78,129-130``),
which resolves it against the model (``utils/order_queryset.py:49-57``).
An unknown name raises ``FieldError`` and the generic 500 envelope; no user
text reaches SQL. The Rust port used to interpolate the value into its
``SELECT``/``ORDER BY``. These tests pin the Django behavior and would hang
on the old port (the payload sleeps in the database).
"""

import time

import pytest

pytestmark = pytest.mark.contract

BASE = "/api/public"
SERVER_ERROR = {"error": "Something went wrong please try again later"}
# The trailing `"issues"."id"` qualification is what makes the injected row
# expression valid in the port's grouped projection; it is still raw SQL from
# an unauthenticated query parameter.
INJECTION = 'id",(SELECT 1 FROM pg_sleep(3)),"issues"."id'


def _issues_url(world):
    return f"{BASE}/anchor/{world['anchor']}/issues/"


def test_issue_list_order_by_rejects_sql_injection(anon_client, world):
    started = time.monotonic()
    response = anon_client.get(_issues_url(world), params={"order_by": INJECTION})
    elapsed = time.monotonic() - started
    assert response.status_code == 500
    assert response.json() == SERVER_ERROR
    assert elapsed < 1.0, f"injected order_by took {elapsed:.2f}s"


@pytest.mark.parametrize("value", ["bogus", "+created_at", "", "name__bogus", "state__bogus"])
def test_issue_list_order_by_unknown_field_is_500(anon_client, world, value):
    response = anon_client.get(_issues_url(world), params={"order_by": value})
    assert response.status_code == 500
    assert response.json() == SERVER_ERROR


def test_issue_list_order_by_valid_fields_still_sort(anon_client, world, seeder, db):
    alpha = seeder.create_state(world["workspace"]["id"], world["project"]["id"], name="Alpha", group="unstarted")
    zeta = seeder.create_state(world["workspace"]["id"], world["project"]["id"], name="Zeta", group="unstarted")
    a = seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], alpha["id"],
        name="order-a", priority="urgent", sequence_id=5,
    )
    z = seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], zeta["id"],
        name="order-z", priority="low", sequence_id=2,
    )
    b = seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], world["state"]["id"],
        name="order-b", priority="high", sequence_id=7,
    )
    # Distinct, stable timestamps: the tie-break is `-created_at`.
    for hours, issue in ((3, a), (2, z), (1, b)):
        db.execute(
            "UPDATE issues SET created_at = now() - make_interval(hours => %s) WHERE id = %s",
            (hours, issue["id"]),
        )

    def names(order_by):
        response = anon_client.get(_issues_url(world), params={"order_by": order_by})
        assert response.status_code == 200, response.text
        return [row["name"] for row in response.json()["results"]]

    assert names("-created_at") == ["Contract issue", "order-b", "order-z", "order-a"]
    assert names("sequence_id") == ["Contract issue", "order-z", "order-a", "order-b"]
    assert names("-sequence_id") == ["order-b", "order-a", "order-z", "Contract issue"]
    assert names("priority") == ["Contract issue", "order-z", "order-b", "order-a"]
    assert names("state__name") == ["order-a", "Contract issue", "order-b", "order-z"]


@pytest.mark.parametrize(
    "value",
    [
        "project__default_assignee",
        "project__workspace",
        "state__project",
        "parent__state",
        "workspace__owner",
    ],
)
def test_issue_list_order_by_relation_fk_field_aliases_are_valid(anon_client, world, value):
    response = anon_client.get(_issues_url(world), params={"order_by": value})
    assert response.status_code == 200, response.text
    assert [row["name"] for row in response.json()["results"]] == ["Contract issue"]
