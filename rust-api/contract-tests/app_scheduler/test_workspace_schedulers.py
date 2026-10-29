"""Workspace scheduler list/create contract."""

import pytest

from _harness.seed import ADMIN
from . import seed_scheduler as seed_s


def sched_list(world):
    return f"/api/workspaces/{world.workspace['slug']}/schedulers/"


SCHEDULER_KEYS = {
    "id", "workspace", "slug", "name", "description", "prompt", "color",
    "source", "is_enabled", "active_binding_count", "created_at",
    "updated_at",
}

# PDASHOSS01-319: slug is read-only and derived from the name. A name whose
# derived slug another active scheduler in the workspace already uses is a
# name field error; exact wire bodies on both backends.
NAME_TAKEN = {
    "name": ["A scheduler with the same name already exists in this workspace."]
}
NAME_NEEDS_ALNUM = {"name": ["Name must contain at least one letter or digit."]}


def test_list_empty(org):
    r = org.request("GET", sched_list(org), "admin")
    assert r.status_code == 200
    assert r.json() == []


def test_create_shape(org):
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"name": "Nightly", "description": "d",
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
        json={"name": "Plain", "prompt": "x"},
    )
    assert r.status_code == 201, r.text
    body = r.json()
    assert body["color"] == "#3b82f6"
    assert body["description"] == ""


def test_create_missing_prompt(org):
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"name": "NoPrompt"},
    )
    assert r.status_code == 400
    assert "prompt" in r.json()


def test_create_bad_color(org):
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"name": "Bad", "prompt": "x", "color": "red"},
    )
    assert r.status_code == 400
    assert r.json() == {
        "color": ["color must be a 7-character hex string like '#3b82f6'"]
    }


@pytest.mark.parametrize(
    ("name", "slug"),
    [
        ("Nightly Bug Scan", "nightly-bug-scan"),
        ("  PR  review!!", "pr-review"),
        ("GDPR \u2014 compliance & audit", "gdpr-compliance-audit"),
        ("Caf\u00e9 cr\u00e8me", "caf-cr-me"),
        ("\u0130stanbul Ops", "i-stanbul-ops"),
        ("A" * 63 + " Bbbb", "a" * 63),
    ],
)
def test_create_derives_slug_from_name(org, name, slug):
    r = org.request(
        "POST", sched_list(org), "admin", json={"name": name, "prompt": "x"},
    )
    assert r.status_code == 201, r.text
    assert r.json()["slug"] == slug


def test_create_ignores_body_slug(org):
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"slug": "client-picked", "name": "Weekly Review", "prompt": "x"},
    )
    assert r.status_code == 201, r.text
    assert r.json()["slug"] == "weekly-review"


@pytest.mark.parametrize("name", ["Dup", "dup", "DUP!!", "  -dup-  "])
def test_create_duplicate_name(org, name):
    payload = {"name": "Dup", "prompt": "x"}
    assert org.request("POST", sched_list(org), "admin", json=payload).status_code == 201
    r = org.request(
        "POST", sched_list(org), "admin", json={"name": name, "prompt": "x"},
    )
    assert r.status_code == 400
    assert r.json() == NAME_TAKEN
    # The rejected write stored nothing: still exactly one scheduler.
    r = org.request("GET", sched_list(org), "admin")
    assert [row["slug"] for row in r.json()] == ["dup"]


def test_create_duplicate_name_accumulates_with_other_errors(org):
    payload = {"name": "Dup", "prompt": "x"}
    assert org.request("POST", sched_list(org), "admin", json=payload).status_code == 201
    # validate_name is field-level, so the conflict accumulates with the
    # other field errors; name is the first writable field, so it lists first.
    r = org.request(
        "POST", sched_list(org), "admin", json={"name": "dup", "color": "red"},
    )
    assert r.status_code == 400
    assert list(r.json().keys()) == ["name", "prompt", "color"]
    assert r.json() == {
        **NAME_TAKEN,
        "prompt": ["This field is required."],
        "color": ["color must be a 7-character hex string like '#3b82f6'"],
    }


@pytest.mark.parametrize("name", ["!!!", "\u65e5\u672c\u8a9e", "--"])
def test_create_name_without_letter_or_digit(org, name):
    r = org.request(
        "POST", sched_list(org), "admin", json={"name": name, "prompt": "x"},
    )
    assert r.status_code == 400
    assert r.json() == NAME_NEEDS_ALNUM
    assert org.request("GET", sched_list(org), "admin").json() == []


def test_create_name_reusable_after_soft_delete(org):
    payload = {"name": "Gone", "prompt": "x"}
    r = org.request("POST", sched_list(org), "admin", json=payload)
    assert r.status_code == 201, r.text
    detail = f"{sched_list(org)}{r.json()['id']}/"
    assert org.request("DELETE", detail, "admin").status_code == 204
    # The tombstoned slug is reusable (the guard only covers active rows).
    r = org.request("POST", sched_list(org), "admin", json=payload)
    assert r.status_code == 201, r.text
    assert r.json()["slug"] == "gone"


def test_create_same_name_other_workspace(org, pg):
    from .conftest import make_world
    other = make_world(pg)
    try:
        payload = {"name": "Shared", "prompt": "x"}
        r = org.request("POST", sched_list(org), "admin", json=payload)
        assert r.status_code == 201, r.text
        # Same name in another workspace is a different scope: allowed.
        r = other.request("POST", sched_list(other), "admin", json=payload)
        assert r.status_code == 201, r.text
    finally:
        other.close()


def test_list_ordered_by_name_with_binding_count(org):
    for name in ("B Second", "A First"):
        r = org.request(
            "POST", sched_list(org), "admin",
            json={"name": name, "prompt": "x"},
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


def test_create_strips_python_whitespace(org):
    # DRF CharField strips with Python str.strip(), which also removes
    # U+001C-U+001F (FS/GS/RS/US); Rust str::trim() (char::is_whitespace)
    # does not. Name/description/prompt must be stripped the same way so
    # the stored name and validate_name see what Django sees.
    name = f"X {org.tag}"
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"name": f"\u001f{name}\u001c", "prompt": "p"},
    )
    assert r.status_code == 201, r.text
    assert r.json()["name"] == name
    # A name that is a single control char strips to empty: DRF blank error.
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"name": "\u001f", "prompt": "p"},
    )
    assert r.status_code == 400, r.text
    assert r.json() == {"name": ["This field may not be blank."]}
    # An invisible character can't bypass the duplicate-name check.
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"name": f"\u001f{name}", "prompt": "p"},
    )
    assert r.status_code == 400, r.text
    assert r.json() == NAME_TAKEN
