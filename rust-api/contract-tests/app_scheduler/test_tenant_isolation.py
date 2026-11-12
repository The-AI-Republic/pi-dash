"""Tenant-isolation contract: no cross-workspace or cross-project leakage.

The permission gate runs before object lookup, so cross-tenant access
surfaces as 403 (not 404) — pinned here deliberately.
"""

from . import seed_scheduler as seed_s
from .conftest import make_world

DENIED = {"error": "You don't have the required permissions."}


def _paths(world):
    slug = world.workspace["slug"]
    pid = world.project["id"]
    return {
        "schedulers": f"/api/workspaces/{slug}/schedulers/",
        "bindings": f"/api/workspaces/{slug}/projects/{pid}/scheduler-bindings/",
        "occurrences": f"/api/workspaces/{slug}/projects/{pid}/scheduler-bindings/occurrences/",
    }


def test_outsider_denied_everywhere(org):
    for name, path in _paths(org).items():
        r = org.request("GET", path, "outsider")
        assert r.status_code == 403, name
        assert r.json() == DENIED, name
    r = org.request(
        "POST", _paths(org)["schedulers"], "outsider",
        json={"slug": "x", "name": "X", "prompt": "x"},
    )
    assert r.status_code == 403
    assert r.json() == DENIED


def test_admin_of_other_workspace_denied(org, pg):
    other = make_world(pg)
    try:
        seed_s.create_scheduler(
            other.conn, workspace_id=other.workspace["id"], slug="o",
            name="O",
        )
        for name, path in _paths(other).items():
            # org's admin has no membership in the other workspace.
            r = org.request("GET", path, "admin")
            assert r.status_code == 403, name
            assert r.json() == DENIED, name
    finally:
        other.close()


def test_unknown_workspace_slug_denied(org):
    r = org.request("GET", "/api/workspaces/ws-does-not-exist/schedulers/", "admin")
    assert r.status_code == 403
    assert r.json() == DENIED


def test_member_of_other_project_denied(org):
    # Same workspace, second project, no membership: project-scoped rows
    # stay invisible.
    from _harness import seed
    second = seed.create_project(
        org.conn, workspace_id=org.workspace["id"], identifier=f"D{org.tag}".upper(),
        name="Second", created_by_id=org.admin["id"],
    )
    seed.add_project_member(
        org.conn, project_id=second["id"], workspace_id=org.workspace["id"],
        user_id=org.admin["id"], role=seed.ADMIN,
    )
    sched = org.request(
        "POST", f"/api/workspaces/{org.workspace['slug']}/schedulers/",
        "admin", json={"slug": "s2", "name": "S2", "prompt": "x"},
    ).json()
    seed_s.create_binding(
        org.conn, workspace_id=org.workspace["id"],
        project_id=second["id"], scheduler_id=sched["id"],
        dtstart=seed_s.hours_ago(1),
    )
    path = (
        f"/api/workspaces/{org.workspace['slug']}"
        f"/projects/{second['id']}/scheduler-bindings/"
    )
    r = org.request("GET", path, "member")
    assert r.status_code == 403
    assert r.json() == DENIED
    # ...while the admin on that project sees exactly its own rows.
    rows = org.request("GET", path, "admin").json()
    assert len(rows) == 1
    assert rows[0]["project"] == second["id"]
