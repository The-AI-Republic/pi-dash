"""Workspace scheduler detail get/patch/delete contract."""

from . import seed_scheduler as seed_s
from .test_workspace_schedulers import SCHEDULER_KEYS, sched_list


def sched_detail(world, sid):
    return f"/api/workspaces/{world.workspace['slug']}/schedulers/{sid}/"


def _create(org, slug="one", name="One"):
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"slug": slug, "name": name, "prompt": "x"},
    )
    assert r.status_code == 201, r.text
    return r.json()


def test_get_shape(org):
    created = _create(org)
    r = org.request("GET", sched_detail(org, created["id"]), "admin")
    assert r.status_code == 200
    assert set(r.json().keys()) == SCHEDULER_KEYS
    assert r.json()["id"] == created["id"]
    assert r.json()["active_binding_count"] == 0


def test_get_unknown_id(org):
    r = org.request(
        "GET", sched_detail(org, "00000000-0000-0000-0000-000000000000"),
        "admin",
    )
    assert r.status_code == 404
    assert r.json() == {"detail": "No Scheduler matches the given query."}


def test_get_id_from_other_workspace(org, pg):
    from .conftest import make_world
    other = make_world(pg)
    try:
        sched = seed_s.create_scheduler(
            other.conn, workspace_id=other.workspace["id"], slug="o",
            name="O",
        )
        # URL slug is ours (permission passes); the row belongs elsewhere.
        r = org.request("GET", sched_detail(org, sched["id"]), "admin")
        assert r.status_code == 404
    finally:
        other.close()


def test_patch_fields(org):
    created = _create(org)
    r = org.request(
        "PATCH", sched_detail(org, created["id"]), "admin",
        json={"name": "Renamed", "color": "#FF0000", "is_enabled": False},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["name"] == "Renamed"
    assert body["color"] == "#ff0000"
    assert body["is_enabled"] is False


def test_patch_bad_color(org):
    created = _create(org)
    r = org.request(
        "PATCH", sched_detail(org, created["id"]), "admin",
        json={"color": "not-a-color"},
    )
    assert r.status_code == 400
    assert "color" in r.json()


def test_patch_duplicate_slug(org):
    _create(org, slug="keep", name="Keep")
    created = _create(org, slug="change", name="Change")
    r = org.request(
        "PATCH", sched_detail(org, created["id"]), "admin",
        json={"slug": "keep"},
    )
    assert r.status_code == 400
    assert r.json() == {"error": "The payload is not valid"}


def test_delete(org):
    created = _create(org)
    r = org.request("DELETE", sched_detail(org, created["id"]), "admin")
    assert r.status_code == 204
    assert org.request("GET", sched_detail(org, created["id"]), "admin").status_code == 404
    assert org.request("GET", sched_list(org), "admin").json() == []


def test_delete_unknown_id(org):
    r = org.request(
        "DELETE", sched_detail(org, "00000000-0000-0000-0000-000000000000"),
        "admin",
    )
    assert r.status_code == 404


def test_delete_soft_deletes_bindings_inline(org):
    created = _create(org)
    seed_s.create_binding(
        org.conn, workspace_id=org.workspace["id"],
        project_id=org.project["id"], scheduler_id=created["id"],
        dtstart=seed_s.hours_ago(1),
    )
    bindings_url = (
        f"/api/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/scheduler-bindings/"
    )
    assert len(org.request("GET", bindings_url, "admin").json()) == 1
    assert org.request("DELETE", sched_detail(org, created["id"]), "admin").status_code == 204
    # The API view of the world is consistent the moment the response
    # returns: no orphan active bindings survive the delete.
    assert org.request("GET", bindings_url, "admin").json() == []
