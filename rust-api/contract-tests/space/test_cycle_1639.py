"""Contract: public issue retrieve picks the newest live cycle link (PIDASHCONV-1639).

Django annotates ``cycle_id=Subquery(CycleIssue.objects.filter(
issue=OuterRef("id"), deleted_at__isnull=True).values("cycle_id")[:1])``
(``space/views/issue.py:608-612``) with no ``.order_by()`` clearing, so
``CycleIssue.Meta.ordering = ("-created_at",)`` (``db/models/cycle.py:124``)
renders ``ORDER BY created_at DESC`` inside the subquery: the newest live
link wins. An issue can hold several live links — the only uniqueness rule
is ``(cycle, issue) WHERE deleted_at IS NULL`` (``cycle.py:113-117``).
The Rust retrieve omitted the ``ORDER BY`` (the list path already has it),
so it returned whichever row Postgres scanned first.
"""

import pytest

pytestmark = pytest.mark.contract

BASE = "/api/public"


def test_issue_retrieve_cycle_id_is_newest_live_link(anon_client, world, seeder):
    ws_id = world["workspace"]["id"]
    project_id = world["project"]["id"]
    owner_id = world["owner"]["id"]
    older = seeder.create_cycle(ws_id, project_id, owner_id, name="Older cycle")
    newer = seeder.create_cycle(ws_id, project_id, owner_id, name="Newer cycle")
    # Older link first: without the ORDER BY, Postgres' heap scan hands
    # the retrieve subquery the older row.
    seeder.create_cycle_issue(
        ws_id, project_id, older["id"], world["issue"]["id"],
        created_at="2024-01-01T00:00:00+00:00",
    )
    seeder.create_cycle_issue(
        ws_id, project_id, newer["id"], world["issue"]["id"],
        created_at="2024-06-01T00:00:00+00:00",
    )
    anchor = world["anchor"]
    issue_id = world["issue"]["id"]
    retrieve = anon_client.get(f"{BASE}/anchor/{anchor}/issues/{issue_id}/")
    assert retrieve.status_code == 200
    assert retrieve.json()["cycle_id"] == newer["id"]
    listing = anon_client.get(f"{BASE}/anchor/{anchor}/issues/")
    assert listing.status_code == 200
    row = next(r for r in listing.json()["results"] if str(r["id"]) == str(issue_id))
    assert row["cycle_id"] == newer["id"]
    assert row["cycle_id"] == retrieve.json()["cycle_id"]
