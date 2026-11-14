"""Contract: published-board grouped per-group totals include every row the
group returns (NEWPIDASH-276, DIFF-37).

Django counts group members with the intake/archived/draft ``count_filter``
(intake status 1/-1/2 or no intake row) while the rows queryset carries no
intake filter, so a pending (-2) or snoozed (0) intake issue renders in
``results`` but contributes zero to ``total_results`` — the header reads 2
with 3 rows beneath it. The Rust port copied the predicate; the fixed
behaviour counts what the rows return, so ``total_results`` equals the
group's row count (and ``total_pages`` derives from that truthful top
count). The ``django=`` arm pins the old short total proved against live
Django; the ``rust=`` arm pins the fixed total.
"""

import pytest

from _harness.diffs import expect

pytestmark = pytest.mark.contract

BASE = "/api/public"


def _issues_url(world):
    return f"{BASE}/anchor/{world['anchor']}/issues/"


def _seed_group(world, seeder, pending_status=0):
    """Two plain issues plus one intake-linked issue, all in the world
    state. Returns the three issue ids (pending-linked last)."""
    workspace_id = world["workspace"]["id"]
    project_id = world["project"]["id"]
    state_id = world["state"]["id"]
    tag = seeder.tag
    plain_a = seeder.create_issue(
        workspace_id, project_id, state_id, name=f"Plain A {tag}", sequence_id=201
    )
    plain_b = seeder.create_issue(
        workspace_id, project_id, state_id, name=f"Plain B {tag}", sequence_id=202
    )
    linked = seeder.create_issue(
        workspace_id, project_id, state_id, name=f"Linked {tag}", sequence_id=203
    )
    seeder.create_intake_issue(
        workspace_id, project_id, world["intake"]["id"], linked["id"], status=pending_status
    )
    return plain_a, plain_b, linked


def _get_group(anon_client, world, params):
    response = anon_client.get(_issues_url(world), params=params)
    assert response.status_code == 200
    body = response.json()
    groups = body["results"]
    assert isinstance(groups, dict)
    bucket = groups[world["state"]["id"]]
    return body, bucket


def test_grouped_total_counts_pending_intake_rows(anon_client, world, seeder):
    # World already carries one plain issue in the state (conftest);
    # add two plain + one pending linked: 4 rows, 3 counted on Django.
    _seed_group(world, seeder, pending_status=-2)
    body, bucket = _get_group(anon_client, world, {"group_by": "state_id"})
    rows = bucket["results"]
    total = bucket["total_results"]
    # Rows include the pending intake issue on both backends.
    assert len(rows) == 4
    # Header semantics: Django 3, fixed 4 (DIFF-37).
    assert total == expect("DIFF-37", django=3, rust=4)
    # Top-level total already counts rows on both backends.
    assert body["total_count"] == 4


def test_grouped_total_counts_snoozed_intake_rows(anon_client, world, seeder):
    _seed_group(world, seeder, pending_status=0)
    _body, bucket = _get_group(anon_client, world, {"group_by": "state_id"})
    assert len(bucket["results"]) == 4
    assert bucket["total_results"] == expect("DIFF-37", django=3, rust=4)


def test_grouped_total_pages_derive_from_truthful_counts(anon_client, world, seeder):
    # 4 rows in one group, 2 per page: Django top count 3 -> 2 pages? No:
    # ceil(3/2)=2 and ceil(4/2)=2 agree, so use 3 rows in the group to
    # separate them. Move to a fresh state with exactly 3 issues: 2 plain
    # plus 1 pending. World state already has 1 issue, so seed a new
    # state with exactly 3.
    workspace_id = world["workspace"]["id"]
    project_id = world["project"]["id"]
    state = seeder.create_state(workspace_id, project_id, name=f"Paging {seeder.tag}")
    state_id = state["id"]
    first = seeder.create_issue(workspace_id, project_id, state_id, name=f"P1 {seeder.tag}", sequence_id=301)
    seeder.create_issue(workspace_id, project_id, state_id, name=f"P2 {seeder.tag}", sequence_id=302)
    linked = seeder.create_issue(workspace_id, project_id, state_id, name=f"P3 {seeder.tag}", sequence_id=303)
    seeder.create_intake_issue(workspace_id, project_id, world["intake"]["id"], linked["id"], status=-2)
    response = anon_client.get(
        _issues_url(world), params={"group_by": "state_id", "per_page": "2"}
    )
    assert response.status_code == 200
    body = response.json()
    bucket = body["results"][state_id]
    assert bucket["total_results"] == expect("DIFF-37", django=2, rust=3)
    # ceil(2/2)=1 page (short: row 3 unreachable by count) vs ceil(3/2)=2.
    assert body["total_pages"] == expect("DIFF-37", django=1, rust=2)
    # First page carries 2 rows either way (window is row-based).
    assert len(bucket["results"]) == 2


def test_subgrouped_totals_count_pending_intake_rows(anon_client, world, seeder):
    _seed_group(world, seeder, pending_status=-2)
    response = anon_client.get(
        _issues_url(world), params={"group_by": "state_id", "sub_group_by": "priority"}
    )
    assert response.status_code == 200
    body = response.json()
    group = body["results"][world["state"]["id"]]
    assert group["total_results"] == expect("DIFF-37", django=3, rust=4)
    cell = group["results"]["none"]
    # All four issues share priority "none" (seeder default + world seed).
    assert cell["total_results"] == expect("DIFF-37", django=3, rust=4)
    assert len(cell["results"]) == 4
