"""Join-requiring filters on the app intake-issue list (PIDASHCONV-1427).

``issue_filters(request.GET, "GET", "issue__")`` (base.py:183) compiles
assignees/state_group/mentions/cycle/module/subscriber into predicates over
joined tables. Django's ORM adds the JOINs; the Rust port must too — the row
query once referenced the joined tables without joining them (500).
"""

import uuid

import pytest

from .conftest import intake_urls

pytestmark = pytest.mark.contract


def _rows(payload):
    if isinstance(payload, dict) and "results" in payload:
        return payload["results"]
    assert isinstance(payload, list), f"expected list envelope, got {payload!r}"[:500]
    return payload


def _link_assignee(seeder, ws, proj, issue_id, user_id):
    row_id = uuid.uuid4().hex
    seeder.db.execute(
        "INSERT INTO issue_assignees (id, workspace_id, project_id, issue_id, assignee_id, created_at, updated_at)"
        " VALUES (%s,%s,%s,%s,%s,now(),now())",
        (row_id, ws, proj, issue_id, user_id),
    )
    seeder.track.add("issue_assignees", row_id)


def _seed_pair(tenant):
    """One intake issue assigned to member A, one unassigned; both triage."""
    seeder = tenant["seeder"]
    ws = tenant["workspace"]["id"]
    proj = tenant["project"]["id"]
    triage = seeder.create_state(ws, proj, name="Triage", group="triage")
    member_a = seeder.create_user()
    seeder.create_workspace_member(ws, member_a["id"])
    seeder.create_project_member(ws, proj, member_a["id"])
    issue_a = seeder.create_issue(ws, proj, triage["id"], name="Assigned", sequence_id=1)
    issue_u = seeder.create_issue(ws, proj, triage["id"], name="Unassigned", sequence_id=2)
    _link_assignee(seeder, ws, proj, issue_a["id"], member_a["id"])
    bridge_a = seeder.create_intake_issue(ws, proj, tenant["intake"]["id"], issue_a["id"])
    bridge_u = seeder.create_intake_issue(ws, proj, tenant["intake"]["id"], issue_u["id"])
    return member_a, bridge_a, bridge_u


def test_list_filter_assignees_returns_only_assigned_row(make_tenant):
    tenant = make_tenant()
    member_a, bridge_a, _bridge_u = _seed_pair(tenant)
    urls = intake_urls(tenant)
    r = tenant["client"].get(urls["intake_issues"], params={"assignees": member_a["id"], "status": "-2"})
    assert r.status_code == 200, r.text[:500]
    rows = _rows(r.json())
    assert [row["id"] for row in rows] == [bridge_a["id"]]


def test_list_filter_state_group_triage_returns_both_rows(make_tenant):
    tenant = make_tenant()
    _member_a, bridge_a, bridge_u = _seed_pair(tenant)
    urls = intake_urls(tenant)
    r = tenant["client"].get(urls["intake_issues"], params={"state_group": "triage"})
    assert r.status_code == 200, r.text[:500]
    rows = _rows(r.json())
    assert {row["id"] for row in rows} == {bridge_a["id"], bridge_u["id"]}
