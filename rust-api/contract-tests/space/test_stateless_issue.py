"""Contract: stateless issues survive the triage exclusion on space reads.

``Issue.issue_objects`` excludes ``state__group='triage'`` null-safely
(PB-0343): Django renders ``NOT (group = 'triage' AND group IS NOT NULL)``,
so issues with ``state_id IS NULL`` are kept by the public issue list, the
grouped totals, the ``sub_issues_count`` annotation and the retrieve
(PIDASHCONV-1634; same trap as PIDASHCONV-1250 in the assistant tools).

Grouped-shape note, verified against live Django main: a stateless issue is
COUNTED in the grouped totals but placed in NO group — ``group_by=state_id``
and ``group_by=state__group`` emit no null/``"None"`` group
(``paginator.py`` ``__query_grouper`` only appends rows whose key is in
``group_by_fields``). The tests pin that exact shape.
"""

import pytest

pytestmark = pytest.mark.contract

BASE = "/api/public"


def _issues_url(world):
    return f"{BASE}/anchor/{world['anchor']}/issues/"


def _grouped_names(body):
    names = []
    for group in body["results"].values():
        names.extend(row["name"] for row in group["results"])
    return names


def test_stateless_issue_in_public_list_and_totals(anon_client, world, seeder):
    stateless = seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], None,
        name="Stateless issue", sequence_id=50,
    )
    response = anon_client.get(_issues_url(world))
    assert response.status_code == 200
    body = response.json()
    rows = {row["name"]: row for row in body["results"]}
    assert "Stateless issue" in rows
    assert rows["Stateless issue"]["id"] == stateless["id"]
    assert rows["Stateless issue"]["state_id"] is None
    assert rows["Stateless issue"]["state__group"] is None
    # World seeds one issue; the stateless one brings the total to two.
    assert body["total_results"] == 2


@pytest.mark.parametrize("axis", ["state_id", "state__group"])
def test_stateless_issue_counted_in_grouped_totals_but_ungrouped(anon_client, world, seeder, axis):
    seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], None,
        name="Stateless issue", sequence_id=50,
    )
    response = anon_client.get(_issues_url(world), params={"group_by": axis})
    assert response.status_code == 200
    body = response.json()
    assert body["total_results"] == 2
    assert "Stateless issue" not in _grouped_names(body)
    assert "None" not in body["results"]


def test_stateless_child_counted_in_intake_sub_issues_count(user_client, world, seeder, db):
    workspace_id = world["workspace"]["id"]
    project_id = world["project"]["id"]
    parent = seeder.create_issue(
        workspace_id, project_id, world["state"]["id"],
        name="Parent issue", sequence_id=60,
    )
    child = seeder.create_issue(
        workspace_id, project_id, None,
        name="Stateless child", sequence_id=61,
    )
    db.execute(
        "UPDATE issues SET parent_id = %s WHERE id = %s",
        (parent["id"], child["id"]),
    )
    seeder.create_intake_issue(
        workspace_id, project_id, world["intake"]["id"], parent["id"]
    )
    url = f"{BASE}/anchor/{world['anchor']}/intakes/{world['intake']['id']}/intake-issues/"
    response = user_client.get(url)
    assert response.status_code == 200
    rows = {row["name"]: row for row in response.json()}
    assert rows["Parent issue"]["sub_issues_count"] == 1


def test_stateless_issue_retrieve_is_200(anon_client, world, seeder):
    stateless = seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], None,
        name="Stateless issue", sequence_id=50,
    )
    response = anon_client.get(f"{_issues_url(world)}{stateless['id']}/")
    assert response.status_code == 200
    assert response.content, "retrieve returned an empty body for a live issue"
    body = response.json()
    assert body["id"] == stateless["id"]
    assert body["state_id"] is None
