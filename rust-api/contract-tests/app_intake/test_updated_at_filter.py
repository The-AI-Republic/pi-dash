"""PDASHOSS01-312: the legacy ``updated_at`` list filter compares the range
against the issue's updated day, not its created day.

One issue is created on DAY_A and last updated on DAY_B; an updated-date
filter for DAY_B must return it and one for DAY_A must not. Both callers of
the shared ``filter_updated_at`` helper are pinned: the intake list
(``issue__`` prefix) and the project work-item list.
"""

import pytest

from .conftest import intake_urls

pytestmark = pytest.mark.contract

DAY_A = "2026-01-05"
DAY_B = "2026-01-12"


def _range(day):
    return f"{day};after,{day};before"


def _rows(payload):
    if isinstance(payload, dict) and "results" in payload:
        return payload["results"]
    assert isinstance(payload, list), repr(payload)[:500]
    return payload


def _stamp(tenant, issue_id):
    tenant["seeder"].db.execute(
        "UPDATE issues SET created_at = %s, updated_at = %s WHERE id = %s",
        (f"{DAY_A}T12:00:00+00:00", f"{DAY_B}T12:00:00+00:00", issue_id),
    )


def test_intake_list_updated_at_filters_the_updated_day(make_tenant):
    tenant = make_tenant()
    url = intake_urls(tenant)["intake_issues"]
    r = tenant["client"].post(url, json={"issue": {"name": "Stale intake", "priority": "none"}})
    assert r.status_code == 200, r.text[:500]
    issue_id = r.json()["issue"]["id"]
    _stamp(tenant, issue_id)

    def listed(day):
        r = tenant["client"].get(url, params={"updated_at": _range(day)})
        assert r.status_code == 200, r.text[:500]
        return {row["issue"]["id"] for row in _rows(r.json())}

    assert issue_id in listed(DAY_B)
    assert issue_id not in listed(DAY_A)


def test_work_item_list_updated_at_filters_the_updated_day(make_tenant):
    tenant = make_tenant()
    workspace_id, project_id = tenant["workspace"]["id"], tenant["project"]["id"]
    state = tenant["seeder"].create_state(workspace_id, project_id, name="Todo", group="unstarted")
    issue_id = tenant["seeder"].create_issue(workspace_id, project_id, state["id"], name="Stale work item")["id"]
    _stamp(tenant, issue_id)
    url = f"/api/workspaces/{tenant['workspace']['slug']}/projects/{project_id}/issues/"

    def listed(day):
        r = tenant["client"].get(url, params={"updated_at": _range(day)})
        assert r.status_code == 200, r.text[:500]
        return {row["id"] for row in _rows(r.json())}

    assert issue_id in listed(DAY_B)
    assert issue_id not in listed(DAY_A)
