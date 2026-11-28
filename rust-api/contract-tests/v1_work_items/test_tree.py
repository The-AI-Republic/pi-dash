"""``GET .../work-items/<id>/tree/`` — children with a state-group rollup.

PDASHOSS01-405 (``IssueTreeAPIEndpoint``, ``orchestration/issue_tree.py``;
Rust ``operations::issue_tree``). Children are created through the API so
both servers see the same rows; the response is compared field by field,
including key order and child order.
"""

from __future__ import annotations

import uuid

import pytest

from .conftest import issue_url

pytestmark = pytest.mark.contract

ISSUE_KEYS = ["id", "identifier", "name", "state", "state_group", "priority"]
CHILD_KEYS = ISSUE_KEYS + ["parent", "depth", "assignees", "active_run"]
GROUPS = ["backlog", "unstarted", "started", "review", "test", "completed", "cancelled"]


def tree_url(world, issue_id, query=""):
    return issue_url(world, issue_id) + "tree/" + query


def ident(issue):
    return issue["url"].rsplit("/", 1)[-1]


@pytest.fixture()
def states(seeder, world):
    ws, project = world["workspace"]["id"], world["project"]["id"]
    return {
        group: seeder.create_state(ws, project, name=f"S-{group}", group=group)["id"]
        for group in ("backlog", "started", "completed", "cancelled")
    }


def make(api, world, name, *, parent=None, state=None):
    body = {"name": name, "priority": "none"}
    if parent:
        body["parent"] = parent
    if state:
        body["state"] = state
    response = api.post(issue_url(world), json=body)
    assert response.status_code == 201, response.text
    return response.json()


def test_tree_rollup_mixed_groups(api, world, states):
    epic = world["issue"]
    epic_id = epic["id"]
    a = make(api, world, "a done", parent=epic_id, state=states["completed"])
    b = make(api, world, "b wip", parent=epic_id, state=states["started"])
    c = make(api, world, "c dropped", parent=epic_id, state=states["cancelled"])
    a1 = make(api, world, "a1 backlog", parent=a["id"], state=states["backlog"])

    response = api.get(tree_url(world, epic_id))
    assert response.status_code == 200, response.text
    body = response.json()
    assert list(body) == ["issue", "recursive", "rollup", "children", "truncated"]
    assert list(body["issue"]) == ISSUE_KEYS
    assert body["issue"]["id"] == epic_id
    assert body["recursive"] is False and body["truncated"] is False
    assert body["rollup"] == {
        "total": 3,
        "done": 2,
        "open": 1,
        "by_state_group": {g: {"started": 1, "completed": 1, "cancelled": 1}.get(g, 0) for g in GROUPS},
    }
    assert list(body["rollup"]["by_state_group"]) == GROUPS
    assert [child["id"] for child in body["children"]] == [a["id"], b["id"], c["id"]]
    for child in body["children"]:
        assert list(child) == CHILD_KEYS
        assert child["depth"] == 1
        assert child["assignees"] == [] and child["active_run"] is None
    assert body["children"][1]["state"] == "S-started"
    assert body["children"][1]["state_group"] == "started"
    assert body["children"][1]["identifier"] == ident(b)

    deep = api.get(tree_url(world, epic_id, "?recursive=true")).json()
    assert deep["recursive"] is True
    assert [(child["id"], child["depth"], child["parent"]) for child in deep["children"]] == [
        (a["id"], 1, ident(epic)),
        (a1["id"], 2, ident(a)),
        (b["id"], 1, ident(epic)),
        (c["id"], 1, ident(epic)),
    ]
    assert deep["rollup"]["total"] == 4
    assert deep["rollup"]["done"] == 2
    assert deep["rollup"]["by_state_group"]["backlog"] == 1


def test_tree_leaf_and_bad_recursive(api, world):
    issue_id = world["issue"]["id"]
    body = api.get(tree_url(world, issue_id)).json()
    assert body["children"] == []
    assert body["rollup"]["total"] == 0
    bad = api.get(tree_url(world, issue_id, "?recursive=maybe"))
    assert bad.status_code == 400
    assert bad.json() == {"error": "recursive must be true or false"}


def test_tree_unknown_issue_404(api, world):
    response = api.get(tree_url(world, "00000000-0000-0000-0000-000000000000"))
    assert response.status_code == 404
    assert response.json() == {"error": "The requested resource does not exist."}


def test_tree_cross_tenant_403(other_api, world):
    response = other_api.get(tree_url(world, world["issue"]["id"]))
    assert response.status_code == 403


def test_tree_child_assignee_and_newest_active_run(api, world, seeder, db, states):
    epic_id = world["issue"]["id"]
    owner = world["owner"]
    ws, project = world["workspace"]["id"], world["project"]["id"]
    wip = make(api, world, "wip", parent=epic_id, state=states["started"])
    idle = make(api, world, "idle", parent=epic_id)
    assignee_id = str(uuid.uuid4())
    db.execute(
        """INSERT INTO issue_assignees
           (id, assignee_id, issue_id, project_id, workspace_id, created_at, updated_at)
           VALUES (%s,%s,%s,%s,%s,now(),now())""",
        (assignee_id, owner["id"], wip["id"], project, ws),
    )
    seeder._put("issue_assignees", assignee_id)
    pod = seeder.create_pod(ws, project)
    seeder.create_agent_run(ws, owner["id"], pod["id"], wip["id"], status="completed")
    run = seeder.create_agent_run(ws, owner["id"], pod["id"], wip["id"], status="running")

    body = api.get(tree_url(world, epic_id)).json()

    by_id = {child["id"]: child for child in body["children"]}
    display_name = db.fetchval("SELECT display_name FROM users WHERE id = %s", (owner["id"],))
    assert by_id[wip["id"]]["assignees"] == [{"id": owner["id"], "display_name": display_name}]
    assert by_id[wip["id"]]["active_run"] == {"id": run["id"], "status": "running"}
    assert by_id[idle["id"]]["assignees"] == []
    assert by_id[idle["id"]]["active_run"] is None


def test_tree_hidden_child_cuts_off_its_subtree(api, world, db):
    epic_id = world["issue"]["id"]
    hidden = make(api, world, "hidden", parent=epic_id)
    make(api, world, "under hidden", parent=hidden["id"])
    shown = make(api, world, "shown", parent=epic_id)
    db.execute("UPDATE issues SET archived_at = now() WHERE id = %s", (hidden["id"],))

    body = api.get(tree_url(world, epic_id, "?recursive=true")).json()
    assert [child["id"] for child in body["children"]] == [shown["id"]]
    assert body["rollup"]["total"] == 1


def test_tree_parent_cycle_terminates(api, world, db):
    epic_id = world["issue"]["id"]
    a = make(api, world, "a", parent=epic_id)
    b = make(api, world, "b", parent=a["id"])
    db.execute("UPDATE issues SET parent_id = %s WHERE id = %s", (b["id"], epic_id))

    body = api.get(tree_url(world, epic_id, "?recursive=true")).json()
    assert [(child["id"], child["depth"]) for child in body["children"]] == [(a["id"], 1), (b["id"], 2)]
    assert body["truncated"] is False


def _clone_children(db, template_id, first, count):
    """Insert ``count`` copies of the ``template_id`` issue under the same
    parent, with sequence ids ``first + 1 ..``: cheaper than 1000 POSTs."""
    columns = [
        row["column_name"]
        for row in db.fetchall(
            """SELECT column_name FROM information_schema.columns
               WHERE table_name = 'issues' AND is_generated = 'NEVER'
               ORDER BY ordinal_position"""
        )
    ]
    override = {"id": "gen_random_uuid()", "sequence_id": f"{first} + g", "name": "'bulk ' || g"}
    select = ", ".join(override.get(name, f's."{name}"') for name in columns)
    names = ", ".join(f'"{name}"' for name in columns)
    db.execute(
        f"INSERT INTO issues ({names}) SELECT {select} FROM issues s, generate_series(1, %s) g WHERE s.id = %s",
        (count, template_id),
    )


@pytest.fixture
def bulk_clones(db):
    """Parents whose ``_clone_children`` rows are deleted on teardown: the
    clones bypass the seeder, so the work-item fixture would leave them."""
    parents = []
    yield parents
    for parent_id in parents:
        db.execute("DELETE FROM issues WHERE parent_id = %s AND name LIKE 'bulk %%'", (parent_id,))


def test_tree_truncated_only_past_node_limit(api, world, db, bulk_clones):
    epic_id = world["issue"]["id"]
    bulk_clones.append(epic_id)
    first = make(api, world, "first", parent=epic_id)
    _clone_children(db, first["id"], 100000, 999)

    at_limit = api.get(tree_url(world, epic_id)).json()
    assert at_limit["truncated"] is False
    assert len(at_limit["children"]) == 1000
    assert at_limit["rollup"]["total"] == 1000

    _clone_children(db, first["id"], 200000, 1)
    past = api.get(tree_url(world, epic_id)).json()
    assert past["truncated"] is True
    assert len(past["children"]) == 1000
    assert past["rollup"]["total"] == 1000
    # The cut keeps the lowest sequence ids.
    assert past["children"][0]["id"] == first["id"]
    assert past["children"][-1]["name"] == "bulk 999"
