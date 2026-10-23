"""Workspace scheduler detail get/patch/delete contract."""

from . import seed_scheduler as seed_s
from .test_workspace_schedulers import NAME_NEEDS_ALNUM, NAME_TAKEN, SCHEDULER_KEYS, sched_list


def sched_detail(world, sid):
    return f"/api/workspaces/{world.workspace['slug']}/schedulers/{sid}/"


def _create(org, name="One"):
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"name": name, "prompt": "x"},
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
    assert body["slug"] == "renamed"  # re-derived from the new name
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


def test_patch_duplicate_name(org):
    _create(org, name="Keep")
    created = _create(org, name="Change")
    r = org.request(
        "PATCH", sched_detail(org, created["id"]), "admin",
        json={"name": "KEEP!"},
    )
    assert r.status_code == 400
    assert r.json() == NAME_TAKEN
    # Nothing saved.
    r = org.request("GET", sched_detail(org, created["id"]), "admin")
    assert (r.json()["name"], r.json()["slug"]) == ("Change", "change")


def test_patch_duplicate_name_accumulates_with_other_errors(org):
    _create(org, name="Keep")
    created = _create(org, name="Change")
    r = org.request(
        "PATCH", sched_detail(org, created["id"]), "admin",
        json={"name": "keep", "prompt": "   "},
    )
    assert r.status_code == 400
    assert list(r.json().keys()) == ["name", "prompt"]
    assert r.json() == {
        **NAME_TAKEN,
        "prompt": ["This field may not be blank."],
    }


def test_patch_name_without_letter_or_digit(org):
    created = _create(org, name="Mine")
    r = org.request(
        "PATCH", sched_detail(org, created["id"]), "admin",
        json={"name": "!!!"},
    )
    assert r.status_code == 400
    assert r.json() == NAME_NEEDS_ALNUM


def test_patch_own_name_variant(org):
    created = _create(org, name="Mine")
    # The own row is excluded from the duplicate check.
    r = org.request(
        "PATCH", sched_detail(org, created["id"]), "admin",
        json={"name": "MINE"},
    )
    assert r.status_code == 200, r.text
    assert (r.json()["name"], r.json()["slug"]) == ("MINE", "mine")


def test_patch_body_slug_ignored(org):
    created = _create(org, name="Mine")
    r = org.request(
        "PATCH", sched_detail(org, created["id"]), "admin",
        json={"slug": "other", "prompt": "new"},
    )
    assert r.status_code == 200, r.text
    assert r.json()["slug"] == "mine"
    assert r.json()["prompt"] == "new"


def test_patch_rename_builtin_keeps_slug(org):
    # ensure_builtin_schedulers matches built-ins by slug, so a rename only
    # changes the display name.
    sched = seed_s.create_scheduler(
        org.conn, workspace_id=org.workspace["id"], slug="security-audit",
        name="Security Audit",
    )
    r = org.request(
        "PATCH", sched_detail(org, sched["id"]), "admin",
        json={"name": "Weekly Sec Sweep"},
    )
    assert r.status_code == 200, r.text
    assert (r.json()["name"], r.json()["slug"]) == (
        "Weekly Sec Sweep", "security-audit",
    )
    r = org.request("GET", sched_detail(org, sched["id"]), "admin")
    assert r.json()["slug"] == "security-audit"


def test_patch_rename_builtin_to_taken_name(org):
    _create(org, name="Keep")
    sched = seed_s.create_scheduler(
        org.conn, workspace_id=org.workspace["id"], slug="security-audit",
        name="Security Audit",
    )
    r = org.request(
        "PATCH", sched_detail(org, sched["id"]), "admin",
        json={"name": "Keep"},
    )
    assert r.status_code == 400
    assert r.json() == NAME_TAKEN


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


def test_patch_strips_python_whitespace(org):
    # Same Python str.strip() semantics on PATCH (U+001C-U+001F included).
    created = _create(org, name="One")
    r = org.request(
        "PATCH", sched_detail(org, created["id"]), "admin",
        json={"name": "\u001fRenamed\u001c"},
    )
    assert r.status_code == 200, r.text
    assert r.json()["name"] == "Renamed"
    r = org.request(
        "PATCH", sched_detail(org, created["id"]), "admin",
        json={"name": "\u001f"},
    )
    assert r.status_code == 400, r.text
    assert r.json() == {"name": ["This field may not be blank."]}
