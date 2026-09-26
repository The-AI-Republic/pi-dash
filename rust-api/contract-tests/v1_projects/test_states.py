"""Contract tests: api-v1 state endpoints (urls/state.py, 2 URL entries).

Covers state list/create and detail get/patch/delete, plus identifier-slug
routing, one denied case and one tenant-isolation case. Note create answers
200 (not 201).
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import db, http  # noqa: E402

KEY = "states"

PAGE_KEYS = {
    "count", "extra_stats", "grouped_by", "next_cursor", "next_page_results",
    "prev_cursor", "prev_page_results", "results", "sub_grouped_by",
    "total_count", "total_pages", "total_results",
}


def base(seed):
    return (f"/api/v1/workspaces/{seed['ws_a']['slug']}"
            f"/projects/{seed['project']['id']}/states")


def test_list_shape(seed, conn):
    tag = db.new_tag()
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag)
    body = http.get(seed["keys"][KEY], base(seed) + "/").json()
    assert set(body.keys()) == PAGE_KEYS
    assert body["total_count"] >= 1
    by_id = {s["id"]: s for s in body["results"]}
    assert st["id"] in by_id
    row = by_id[st["id"]]
    for k in ("id", "name", "color", "group", "sequence", "default",
              "project", "workspace", "created_at", "updated_at"):
        assert k in row, f"state list item missing {k}"
    assert row["is_triage"] is False


def test_list_by_slug(seed):
    body = http.get(
        seed["keys"][KEY],
        f"/api/v1/workspaces/{seed['ws_a']['slug']}"
        f"/projects/{seed['project']['identifier']}/states/").json()
    assert body["total_count"] >= 1


def test_create(seed):
    tag = db.new_tag()
    name = f"CT State {tag}"
    # Create answers 200, not 201.
    body = http.post(seed["keys"][KEY], base(seed) + "/",
                     json={"name": name, "color": "#00ff00", "group": "backlog",
                           "sequence": 1000.0},
                     expect=200).json()
    assert body["name"] == name
    assert body["group"] == "backlog"
    assert body["default"] is False


def test_create_conflicts(seed, conn):
    tag = db.new_tag()
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag)
    # Same name -> 409 with the surviving row's id.
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"name": st["name"], "color": "#00ff00", "group": "backlog"},
                  expect=409)
    assert r.json() == {
        "error": "State with the same name already exists in the project",
        "id": st["id"],
    }
    # Triage group can never be created through the API.
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"name": f"CT Tri {tag}", "color": "#00ff00", "group": "triage"},
                  expect=400)
    assert r.json() == {"non_field_errors": ["Cannot create triage state"]}
    # Same external id + source -> 409 with the surviving row's id.
    ext = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag + "e")
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE states SET external_id = 'EXT-1', external_source = 'GH' WHERE id = %s",
            (ext["id"],),
        )
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"name": f"CT Ext {tag}", "color": "#00ff00", "group": "backlog",
                        "external_id": "EXT-1", "external_source": "GH"},
                  expect=409)
    assert r.json() == {
        "error": "State with the same external id and external source already exists",
        "id": ext["id"],
    }


def test_retrieve(seed, conn):
    tag = db.new_tag()
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag)
    body = http.get(seed["keys"][KEY], f"{base(seed)}/{st['id']}/").json()
    assert body["id"] == st["id"]
    assert body["name"] == st["name"]


def test_patch(seed, conn):
    tag = db.new_tag()
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag)
    body = http.patch(seed["keys"][KEY], f"{base(seed)}/{st['id']}/",
                      json={"color": "#0000ff"}).json()
    assert body["color"] == "#0000ff"
    assert body["id"] == st["id"]
    # External-id clash on update -> 409.
    other = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag + "o")
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE states SET external_id = 'EXT-2', external_source = 'GH' WHERE id = %s",
            (other["id"],),
        )
    r = http.patch(seed["keys"][KEY], f"{base(seed)}/{st['id']}/",
                   json={"external_id": "EXT-2", "external_source": "GH"},
                   expect=409)
    assert r.json()["error"] == (
        "State with the same external id and external source already exists")


def test_delete_empty(seed, conn):
    tag = db.new_tag()
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag)
    http.delete(seed["keys"][KEY], f"{base(seed)}/{st['id']}/")
    http.get(seed["keys"][KEY], f"{base(seed)}/{st['id']}/", expect=404)


def test_delete_default_400(seed, conn):
    tag = db.new_tag()
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag)
    with conn.cursor() as cur:
        cur.execute("UPDATE states SET \"default\" = true WHERE id = %s", (st["id"],))
    r = http.delete(seed["keys"][KEY], f"{base(seed)}/{st['id']}/", expect=400)
    assert r.json() == {"error": "Default state cannot be deleted"}


def test_denied_outsider(seed):
    http.get(seed["keys"]["outsider"], base(seed) + "/", expect=403)
    http.post(seed["keys"]["outsider"], base(seed) + "/",
              json={"name": "CT Nope", "color": "#fff", "group": "backlog"},
              expect=403)


def test_isolation_other_workspace(seed, conn):
    tag = db.new_tag()
    st_b = db.create_state(conn, seed["ws_b"]["id"], seed["project_b"]["id"], tag)
    db.add_project_member(conn, seed["ws_b"]["id"], seed["project_b"]["id"],
                          seed["owner"]["id"], db.ADMIN)
    try:
        # owner (admin in both workspaces now) lists ws_b: sees only ws_b states.
        body = http.get(
            seed["keys"][KEY],
            f"/api/v1/workspaces/{seed['ws_b']['slug']}"
            f"/projects/{seed['project_b']['id']}/states/").json()
        ids = {s["id"] for s in body["results"]}
        assert st_b["id"] in ids
        # ... and ws_a's states never leak into ws_b's list.
        body_a = http.get(seed["keys"][KEY], base(seed) + "/").json()
        assert st_b["id"] not in {s["id"] for s in body_a["results"]}
    finally:
        with conn.cursor() as cur:
            cur.execute(
                "DELETE FROM project_members WHERE project_id = %s AND member_id = %s",
                (seed["project_b"]["id"], seed["owner"]["id"]),
            )
