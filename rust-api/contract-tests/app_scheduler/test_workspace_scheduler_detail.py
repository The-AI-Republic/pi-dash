"""Workspace scheduler detail get/patch/delete contract."""

from _harness.diffs import expect, is_rust
from . import seed_scheduler as seed_s
from .test_workspace_schedulers import (
    SCHEDULER_KEYS,
    SLUG_FORMAT,
    SLUG_TAKEN,
    sched_list,
)


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
    assert r.json() == SLUG_TAKEN


def test_patch_duplicate_slug_accumulates_with_other_errors(org):
    _create(org, slug="keep", name="Keep")
    created = _create(org, slug="change", name="Change")
    r = org.request(
        "PATCH", sched_detail(org, created["id"]), "admin",
        json={"slug": "keep", "name": "   "},
    )
    assert r.status_code == 400
    assert list(r.json().keys()) == ["slug", "name"]
    assert r.json() == {
        **SLUG_TAKEN,
        "name": ["This field may not be blank."],
    }


def test_patch_keep_own_slug(org):
    created = _create(org, slug="mine", name="Mine")
    # Re-saving the row's own slug excludes self: allowed.
    r = org.request(
        "PATCH", sched_detail(org, created["id"]), "admin",
        json={"slug": "mine", "name": "Renamed"},
    )
    assert r.status_code == 200, r.text
    assert r.json()["slug"] == "mine"
    assert r.json()["name"] == "Renamed"


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


def test_patch_rename_to_badly_formatted_slug_rejected_on_rust(org):
    # DIFF-34: a rename to a badly formatted slug is Rust-only rejected;
    # Django's validate_slug has no format rule.
    for bad in ["Bad Slug", "Nightly", "nightly_scan"]:
        created = _create(org, slug=f"src-{bad[:2].lower()}-x", name="Src")
        # Fresh per-case row: re-create under a valid slug first.
        r = org.request(
            "PATCH", sched_detail(org, created["id"]), "admin",
            json={"slug": bad},
        )
        assert r.status_code == expect("DIFF-34", django=200, rust=400), r.text
        if is_rust():
            assert r.json() == SLUG_FORMAT
            # The refused rename wrote nothing.
            r = org.request("GET", sched_detail(org, created["id"]), "admin")
            assert r.status_code == 200
            assert r.json()["slug"] == created["slug"]
        else:
            assert r.json()["slug"] == bad
        # Clean up the row (valid or badly named) for the next case.
        assert org.request(
            "DELETE", sched_detail(org, created["id"]), "admin"
        ).status_code == 204


def test_patch_leaving_stored_bad_slug_unchanged_saves(org):
    # NEWPIDASH-257 grandfather: a stored slug that already breaks the
    # format rule (seeded straight into the DB, as a pre-existing row
    # would be) must not block edits that leave it unchanged — on both
    # backends. Django has no format rule at all; Rust skips the check
    # when the re-sent slug equals the stored one.
    bad = "Bad Slug"
    sched = seed_s.create_scheduler(
        org.conn, workspace_id=org.workspace["id"], slug=bad,
        name="Legacy",
    )
    r = org.request("GET", sched_detail(org, sched["id"]), "admin")
    assert r.status_code == 200, r.text
    # Untouched slug (absent): a name-only edit saves.
    r = org.request(
        "PATCH", sched_detail(org, sched["id"]), "admin",
        json={"name": "Renamed"},
    )
    assert r.status_code == 200, r.text
    assert r.json()["slug"] == bad
    assert r.json()["name"] == "Renamed"
    # Re-sent verbatim: saves.
    r = org.request(
        "PATCH", sched_detail(org, sched["id"]), "admin",
        json={"slug": bad, "description": "touched"},
    )
    assert r.status_code == 200, r.text
    assert r.json()["slug"] == bad
    # Renaming it away to a valid slug works.
    r = org.request(
        "PATCH", sched_detail(org, sched["id"]), "admin",
        json={"slug": "now-valid"},
    )
    assert r.status_code == 200, r.text
    assert r.json()["slug"] == "now-valid"


def test_patch_strips_python_whitespace(org):
    # Same Python str.strip() semantics on PATCH (U+001C-U+001F included).
    created = _create(org, slug="p1", name="One")
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
