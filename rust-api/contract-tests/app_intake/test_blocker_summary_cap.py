"""Blocker-summary cap keeps open items first (PIDASHCONV-1437).

Django ``blockers._summary_list`` orders by ``_resolved`` (open first)
and only then slices ``[:100]``, so open blockers are never the ones
cut. The app_intake Rust port capped in SQL before partitioning
open-first, dropping an open blocker that sorts after 100 completed
ones. Seeds 100 completed blockers plus 1 open one sorting last and
asserts the open one appears — first — in ``blocked_by`` (and, via the
shared summary path, in ``blocking``).
"""

import pytest

from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract

DONE_FIRST_SEQ = 1001
DONE_COUNT = 100
OPEN_SEQ = DONE_FIRST_SEQ + DONE_COUNT


def _seed_blockers(tenant, main_issue_id):
    """100 completed + 1 open blockers, the open one sorting last.

    Same-project rows, so ``(identifier, sequence_id)`` order puts the
    open blocker after every completed one. Each row is also linked as
    a dependent, so the ``blocking`` half is covered by the same seed.
    """
    seeder = tenant["seeder"]
    workspace_id = tenant["workspace"]["id"]
    project_id = tenant["project"]["id"]
    open_state = seeder.create_state(
        workspace_id, project_id, name="Open", group="started"
    )
    done_state = seeder.create_state(
        workspace_id, project_id, name="Done", group="completed"
    )
    for seq in range(DONE_FIRST_SEQ, DONE_FIRST_SEQ + DONE_COUNT):
        blocker = seeder.create_issue(
            workspace_id, project_id, done_state["id"],
            name=f"done-{seq}", sequence_id=seq,
        )
        seeder.create_issue_relation(
            workspace_id, project_id, main_issue_id, blocker["id"],
            relation_type="blocked_by",
        )
        seeder.create_issue_relation(
            workspace_id, project_id, blocker["id"], main_issue_id,
            relation_type="blocked_by",
        )
    open_blocker = seeder.create_issue(
        workspace_id, project_id, open_state["id"],
        name="open-last", sequence_id=OPEN_SEQ,
    )
    seeder.create_issue_relation(
        workspace_id, project_id, main_issue_id, open_blocker["id"],
        relation_type="blocked_by",
    )
    seeder.create_issue_relation(
        workspace_id, project_id, open_blocker["id"], main_issue_id,
        relation_type="blocked_by",
    )
    return open_blocker


def test_blocked_by_cap_keeps_open_first(make_tenant):
    tenant = make_tenant()
    urls = intake_urls(tenant)
    created = create_intake_issue(
        tenant["client"], urls["intake_issues"], name="CT1437 main"
    )
    main_issue_id = created["issue"]["id"]
    _seed_blockers(tenant, main_issue_id)

    r = tenant["client"].get(f"{urls['intake_issues']}{main_issue_id}/")
    assert r.status_code == 200, r.text[:500]
    issue = r.json()["issue"]
    blocked_by = issue["relations_summary"]["blocked_by"]

    assert len(blocked_by) == 100
    identifiers = [item["identifier"] for item in blocked_by]
    project_ident = tenant["project"]["identifier"]
    assert identifiers.count(f"{project_ident}-{OPEN_SEQ}") == 1
    assert blocked_by[0]["identifier"] == f"{project_ident}-{OPEN_SEQ}"
    assert blocked_by[0]["state_group"] == "started"
    assert set(blocked_by[0]) == {"identifier", "state", "state_group"}
    assert issue["has_open_blockers"] is True


def test_blocking_cap_keeps_open_first(make_tenant):
    tenant = make_tenant()
    urls = intake_urls(tenant)
    created = create_intake_issue(
        tenant["client"], urls["intake_issues"], name="CT1437 main"
    )
    main_issue_id = created["issue"]["id"]
    _seed_blockers(tenant, main_issue_id)

    r = tenant["client"].get(f"{urls['intake_issues']}{main_issue_id}/")
    assert r.status_code == 200, r.text[:500]
    issue = r.json()["issue"]
    blocking = issue["relations_summary"]["blocking"]

    assert len(blocking) == 100
    project_ident = tenant["project"]["identifier"]
    assert blocking[0]["identifier"] == f"{project_ident}-{OPEN_SEQ}"
    assert blocking[0]["state_group"] == "started"
