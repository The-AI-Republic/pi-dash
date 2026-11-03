"""Contract test: the intake-issue list renders duplicate_to (PIDASHCONV-1434).

Marks an intake issue as a duplicate of X (PATCH status=2 + duplicate_to=X),
then lists with status=2 and asserts the row carries duplicate_to == X.
"""

import pytest

from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract


def test_intake_issue_list_renders_duplicate_to(make_tenant):
    tenant = make_tenant()
    urls = intake_urls(tenant)
    workspace_id = tenant["workspace"]["id"]
    project_id = tenant["project"]["id"]
    # duplicate_to binds the triage-hiding default manager, so the target
    # must be a non-triage issue: seed one in a backlog state directly
    # (an intake-created issue as target 400s — see
    # test_patch_triage_duplicate_rejected).
    state = tenant["seeder"].create_state(
        workspace_id, project_id, name="Dupe target"
    )
    target = tenant["seeder"].create_issue(workspace_id, project_id, state["id"])
    created = create_intake_issue(
        tenant["client"], urls["intake_issues"], name="Dupe list probe"
    )
    marked = tenant["client"].patch(
        f"{urls['intake_issues']}{created['issue']['id']}/",
        json={"status": 2, "duplicate_to": target["id"]},
    )
    assert marked.status_code == 200, marked.text[:500]
    assert marked.json()["duplicate_to"] == target["id"]
    listed = tenant["client"].get(urls["intake_issues"], params={"status": 2})
    assert listed.status_code == 200, listed.text[:500]
    body = listed.json()
    assert isinstance(body, dict) and "results" in body, body
    by_id = {row["id"]: row for row in body["results"]}
    assert created["id"] in by_id, f"marked row missing from status=2 list: {body!r}"[:500]
    row = by_id[created["id"]]
    assert row["status"] == 2
    assert row["duplicate_to"] == target["id"]
