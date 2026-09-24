"""Workspace scheduler list/create contract."""

from _harness.seed import ADMIN
from . import seed_scheduler as seed_s


def sched_list(world):
    return f"/api/workspaces/{world.workspace['slug']}/schedulers/"


SCHEDULER_KEYS = {
    "id", "workspace", "slug", "name", "description", "prompt", "color",
    "source", "is_enabled", "active_binding_count", "created_at",
    "updated_at",
}


def test_list_empty(org):
    r = org.request("GET", sched_list(org), "admin")
    assert r.status_code == 200
    assert r.json() == []


def test_create_shape(org):
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"slug": "nightly", "name": "Nightly", "description": "d",
              "prompt": "Scan.", "color": "#10B981"},
    )
    assert r.status_code == 201, r.text
    body = r.json()
    assert set(body.keys()) == SCHEDULER_KEYS
    assert body["workspace"] == org.workspace["id"]
    assert body["slug"] == "nightly"
    assert body["name"] == "Nightly"
    assert body["description"] == "d"
    assert body["prompt"] == "Scan."
    assert body["color"] == "#10b981"  # canonicalised to lowercase
    assert body["source"] == "builtin"
    assert body["is_enabled"] is True
    assert body["active_binding_count"] == 0
    assert body["created_at"].endswith("Z")
    assert body["updated_at"].endswith("Z")


def test_create_defaults(org):
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"slug": "plain", "name": "Plain", "prompt": "x"},
    )
    assert r.status_code == 201, r.text
    body = r.json()
    assert body["color"] == "#3b82f6"
    assert body["description"] == ""


def test_create_missing_prompt(org):
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"slug": "noprompt", "name": "NoPrompt"},
    )
    assert r.status_code == 400
    assert "prompt" in r.json()


def test_create_bad_color(org):
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"slug": "bad", "name": "Bad", "prompt": "x", "color": "red"},
    )
    assert r.status_code == 400
    assert r.json() == {
        "color": ["color must be a 7-character hex string like '#3b82f6'"]
    }


def test_create_duplicate_slug(org):
    payload = {"slug": "dup", "name": "Dup", "prompt": "x"}
    assert org.request("POST", sched_list(org), "admin", json=payload).status_code == 201
    r = org.request("POST", sched_list(org), "admin", json=payload)
    assert r.status_code == 400


def test_list_ordered_by_name_with_binding_count(org):
    for slug, name in (("b-second", "B Second"), ("a-first", "A First")):
        r = org.request(
            "POST", sched_list(org), "admin",
            json={"slug": slug, "name": name, "prompt": "x"},
        )
        assert r.status_code == 201
    sched = seed_s.create_scheduler(
        org.conn, workspace_id=org.workspace["id"], slug="bound",
        name="M Middle",
    )
    seed_s.create_binding(
        org.conn, workspace_id=org.workspace["id"],
        project_id=org.project["id"], scheduler_id=sched["id"],
        dtstart=seed_s.hours_ago(1),
    )
    r = org.request("GET", sched_list(org), "admin")
    assert r.status_code == 200
    names = [row["name"] for row in r.json()]
    assert names == ["A First", "B Second", "M Middle"]
    counts = {row["name"]: row["active_binding_count"] for row in r.json()}
    assert counts == {"A First": 0, "B Second": 0, "M Middle": 1}
