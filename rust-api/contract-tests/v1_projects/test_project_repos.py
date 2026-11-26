"""Contract tests: project repository list on api-v1 (NEWPIDASH-409, DIFF-69).

``settings.repos`` holds the repo list; detail reads merge it with the
legacy ``repo_url``/``base_branch`` columns (copying a legacy-only
repo into the list on Rust); writes go through the settings fragment
(full-list replace) or the legacy columns, with the columns and the
list kept consistent (double delete, edit-follow). Django has no
``repos`` key: it stores the settings blob unvalidated and never
merges, copies or syncs, so every repo behaviour below pins both
arms. ``repo_url``/``base_branch`` render unchanged on both.
"""

import sys
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import db, http  # noqa: E402
from _harness.diffs import expect, is_rust  # noqa: E402

KEY = "projects"

REPO_A = "https://github.com/acme/web"
REPO_B = "https://github.com/acme/api"


def base(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects"


def _entry(url, provider="github", branch="main"):
    return {
        "id": str(uuid.uuid4()),
        "url": url,
        "provider": provider,
        "base_branch": branch,
    }


def _owned_project(seed, conn, suffix):
    pid = db.create_project(conn, seed["ws_a"]["id"], seed["tag"] + suffix)["id"]
    db.add_project_member(conn, seed["ws_a"]["id"], pid, seed["owner"]["id"], db.ADMIN)
    return pid


def test_detail_read_repos_empty_by_default(seed):
    url = base(seed) + f"/{seed['project']['id']}/"
    r = http.get(seed["keys"][KEY], url)
    body = r.json()
    assert body["repo_url"] == ""
    assert body["base_branch"] == ""
    assert body["settings"] == expect(
        "DIFF-53",
        django={},
        rust={
            "issue_run_policy": "creator_and_assignees",
            "agent_stages": {"mode": "auto", "review": True, "test": True},
            "repos": [],
        },
    )
    if is_rust():
        assert body["repos"] == []
    else:
        assert "repos" not in body


def test_legacy_repo_copies_to_settings_on_first_read(seed, conn):
    pid = _owned_project(seed, conn, "c")
    url = base(seed) + f"/{pid}/"
    http.patch(
        seed["keys"][KEY], url,
        json={"repo_url": REPO_A, "base_branch": "trunk"}, expect=200,
    )
    body = http.get(seed["keys"][KEY], url).json()
    assert body["repo_url"] == REPO_A
    assert body["base_branch"] == "trunk"
    if is_rust():
        assert len(body["repos"]) == 1
        entry = body["repos"][0]
        uuid.UUID(entry["id"])
        assert entry["url"] == REPO_A
        assert entry["provider"] == "github"
        assert entry["base_branch"] == "trunk"
        assert body["settings"]["repos"] == body["repos"]
    else:
        assert "repos" not in body
        assert body["settings"] == {}


def test_settings_patch_replace_clears_legacy_when_entry_removed(seed, conn):
    pid = _owned_project(seed, conn, "r")
    url = base(seed) + f"/{pid}/"
    entry = _entry(REPO_A)
    http.patch(seed["keys"][KEY], url, json={"settings": {"repos": [entry]}}, expect=200)
    http.patch(seed["keys"][KEY], url, json={"settings": {"repos": []}}, expect=200)
    body = http.get(seed["keys"][KEY], url).json()
    if is_rust():
        assert body["repos"] == []
        assert body["repo_url"] == ""
        assert body["settings"]["repos"] == []
    else:
        assert "repos" not in body
        assert body["repo_url"] == ""
        assert body["settings"] == {}


def test_settings_patch_duplicate_forms_are_400(seed, conn):
    pid = _owned_project(seed, conn, "d")
    url = base(seed) + f"/{pid}/"
    first = _entry(REPO_A)
    second = _entry("git@github.com:acme/web.git")
    with http.client(seed["keys"][KEY]) as c:
        r = c.patch(url, json={"settings": {"repos": [first, second]}})
    assert r.status_code == expect("DIFF-69", django=200, rust=400)
    if is_rust():
        assert r.json() == {
            "settings": [
                "invalid value for repos: "
                "entry 1 is a duplicate of another repository in the list"
            ]
        }


def test_legacy_patch_updates_and_removes_entry(seed, conn):
    pid = _owned_project(seed, conn, "l")
    url = base(seed) + f"/{pid}/"
    http.patch(seed["keys"][KEY], url, json={"repo_url": REPO_A}, expect=200)
    entry = _entry(REPO_A)
    http.patch(seed["keys"][KEY], url, json={"settings": {"repos": [entry]}}, expect=200)
    http.patch(seed["keys"][KEY], url, json={"repo_url": REPO_B}, expect=200)
    body = http.get(seed["keys"][KEY], url).json()
    assert body["repo_url"] == REPO_B
    if is_rust():
        assert [row["url"] for row in body["repos"]] == [REPO_B]
        assert body["repos"][0]["id"] == entry["id"]
    else:
        assert "repos" not in body
    http.patch(seed["keys"][KEY], url, json={"repo_url": ""}, expect=200)
    body = http.get(seed["keys"][KEY], url).json()
    assert body["repo_url"] == ""
    if is_rust():
        assert body["repos"] == []
    else:
        assert "repos" not in body


def test_legacy_patch_to_other_entry_is_400(seed, conn):
    pid = _owned_project(seed, conn, "e")
    url = base(seed) + f"/{pid}/"
    http.patch(seed["keys"][KEY], url, json={"repo_url": REPO_A}, expect=200)
    http.patch(
        seed["keys"][KEY], url,
        json={"settings": {"repos": [_entry(REPO_A), _entry(REPO_B)]}}, expect=200,
    )
    with http.client(seed["keys"][KEY]) as c:
        r = c.patch(url, json={"repo_url": "git@github.com:acme/api.git"})
    assert r.status_code == expect("DIFF-69", django=200, rust=400)
    if is_rust():
        assert r.json() == {
            "repo_url": [
                "This repository is already in the project's repository list."
            ]
        }


def test_create_with_repos_defaults_software(seed):
    tag = db.new_tag()
    entry = _entry(REPO_A)
    r = http.post(
        seed["keys"][KEY], base(seed) + "/",
        json={
            "name": "CT Repos %s" % tag,
            "identifier": "G%s" % tag.upper()[:11],
            "settings": {"repos": [entry]},
        },
        expect=201,
    )
    body = r.json()
    assert body["default_work_type"] == expect(
        "DIFF-69", django="general", rust="software"
    )
    if is_rust():
        assert body["repos"] == [entry]
    else:
        assert "repos" not in body
