"""Contract tests: project repository list (NEWPIDASH-409, DIFF-69).

``settings.repos`` holds the repo list; reads merge it with the legacy
``repo_url``/``base_branch`` columns (copying a legacy-only repo into
the list on Rust); writes go through the settings fragment
(full-list replace) or the legacy columns, with the columns and the
list kept consistent (double delete, edit-follow). Django has no
``repos`` key: it stores the settings blob unvalidated and never
merges, copies or syncs, so every repo behaviour below pins both
arms. ``repo_url``/``base_branch`` render unchanged on both.
"""

import uuid

from _harness.diffs import expect, is_rust

REPO_A = "https://github.com/acme/web"
REPO_B = "https://github.com/acme/api"


def _entry(url, provider="github", branch="main"):
    return {
        "id": str(uuid.uuid4()),
        "url": url,
        "provider": provider,
        "base_branch": branch,
    }


def _post_project(client, ws, repo_url="", base_branch="main", settings=None):
    tag = uuid.uuid4().hex[:8].upper()
    body = {"name": "Repos %s" % tag, "identifier": "R%s" % tag[:9]}
    if repo_url or base_branch != "main":
        body["repo_url"] = repo_url
    if base_branch != "main":
        body["base_branch"] = base_branch
    if settings is not None:
        body["settings"] = settings
    resp = client.post(f"/api/workspaces/{ws['slug']}/projects/", json=body)
    assert resp.status_code == 201, resp.text[:500]
    return resp.json()


def _full_settings(policy="creator_and_assignees", repos=None):
    return {
        "issue_run_policy": policy,
        "agent_stages": {"mode": "auto", "review": True, "test": True},
        "repos": [] if repos is None else repos,
    }


def test_retrieve_repos_empty_by_default(world):
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    resp = client.get(url)
    assert resp.status_code == 200, resp.text[:500]
    body = resp.json()
    assert body["repo_url"] == ""
    assert body["base_branch"] == "main"
    assert body["settings"] == expect(
        "DIFF-53",
        django={},
        rust=_full_settings(),
    )
    if is_rust():
        assert body["repos"] == []
    else:
        assert "repos" not in body


def test_legacy_repo_copies_to_settings_on_first_read(world):
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    resp = client.patch(url, json={"repo_url": REPO_A, "base_branch": "trunk"})
    assert resp.status_code == 200, resp.text[:500]
    resp = client.get(url)
    assert resp.status_code == 200, resp.text[:500]
    body = resp.json()
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
        first_id = entry["id"]
        # A second read copies nothing: the same single entry.
        again = client.get(url).json()
        assert len(again["repos"]) == 1
        assert again["repos"][0]["id"] == first_id
    else:
        assert "repos" not in body
        assert body["settings"] == {}


def test_settings_patch_replace_clears_legacy_when_entry_removed(world):
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    client.patch(url, json={"repo_url": REPO_A})
    entry = _entry(REPO_A)
    resp = client.patch(url, json={"settings": {"repos": [entry]}})
    assert resp.status_code == 200, resp.text[:500]
    # Remove the legacy-matching entry through the list: the columns
    # clear on Rust (double delete); Django keeps the columns.
    resp = client.patch(url, json={"settings": {"repos": []}})
    assert resp.status_code == 200, resp.text[:500]
    body = client.get(url).json()
    if is_rust():
        assert body["repos"] == []
        assert body["repo_url"] == ""
        assert body["base_branch"] == ""
        assert body["settings"]["repos"] == []
    else:
        assert "repos" not in body
        assert body["repo_url"] == REPO_A
        assert body["settings"] == {"repos": []}
    # It does not come back on the next read.
    again = client.get(url).json()
    if is_rust():
        assert again["repos"] == []
        assert again["repo_url"] == ""


def test_settings_patch_edit_updates_columns(world):
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    client.patch(url, json={"repo_url": REPO_A})
    entry = _entry(REPO_A)
    client.patch(url, json={"settings": {"repos": [entry]}})
    edited = dict(entry, url=REPO_B, base_branch="trunk")
    resp = client.patch(url, json={"settings": {"repos": [edited]}})
    assert resp.status_code == 200, resp.text[:500]
    body = client.get(url).json()
    if is_rust():
        # Exactly one entry, with the new URL; the columns followed it.
        assert [row["url"] for row in body["repos"]] == [REPO_B]
        assert body["repos"][0]["id"] == entry["id"]
        assert body["repo_url"] == REPO_B
        assert body["base_branch"] == "trunk"
    else:
        assert "repos" not in body
        assert body["repo_url"] == REPO_A


def test_settings_patch_duplicate_forms_are_400(world):
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    first = _entry(REPO_A)
    second = _entry("git@github.com:acme/web.git")
    resp = client.patch(url, json={"settings": {"repos": [first, second]}})
    assert resp.status_code == expect("DIFF-69", django=200, rust=400)
    if is_rust():
        assert resp.json() == {
            "settings": [
                "invalid value for repos: "
                "entry 1 is a duplicate of another repository in the list"
            ]
        }


def test_settings_patch_bad_values_are_400(world):
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    cases = [
        (
            _entry("not a repo"),
            "invalid value for repos: "
            "entry 0 url must be a valid repository URL",
        ),
        (
            _entry(REPO_A, provider="bitbucket"),
            "invalid value for repos: "
            "entry 0 provider must be one of github, gitlab, other",
        ),
        (
            _entry(REPO_A, branch="no spaces here"),
            "invalid value for repos: entry 0 base_branch: "
            "Branch name may contain only letters, numbers, and . _ / -",
        ),
    ]
    for entry, detail in cases:
        resp = client.patch(url, json={"settings": {"repos": [entry]}})
        assert resp.status_code == expect("DIFF-69", django=200, rust=400), entry
        if is_rust():
            assert resp.json() == {"settings": [detail]}, entry


def test_legacy_patch_updates_matching_entry(world):
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    client.patch(url, json={"repo_url": REPO_A})
    entry = _entry(REPO_A)
    client.patch(url, json={"settings": {"repos": [entry]}})
    resp = client.patch(url, json={"repo_url": REPO_B})
    assert resp.status_code == 200, resp.text[:500]
    body = client.get(url).json()
    assert body["repo_url"] == REPO_B
    if is_rust():
        assert [row["url"] for row in body["repos"]] == [REPO_B]
        assert body["repos"][0]["id"] == entry["id"]
        assert body["settings"]["repos"] == body["repos"]
    else:
        assert "repos" not in body
        assert body["settings"] == {"repos": [entry]}


def test_legacy_patch_empty_url_removes_entry(world):
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    client.patch(url, json={"repo_url": REPO_A})
    entry = _entry(REPO_A)
    client.patch(url, json={"settings": {"repos": [entry]}})
    resp = client.patch(url, json={"repo_url": ""})
    assert resp.status_code == 200, resp.text[:500]
    body = client.get(url).json()
    assert body["repo_url"] == ""
    if is_rust():
        assert body["repos"] == []
        assert body["settings"]["repos"] == []
    else:
        assert "repos" not in body
        assert body["settings"] == {"repos": [entry]}


def test_legacy_patch_to_other_entry_is_400(world):
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    client.patch(url, json={"repo_url": REPO_A})
    first = _entry(REPO_A)
    second = _entry(REPO_B)
    client.patch(url, json={"settings": {"repos": [first, second]}})
    resp = client.patch(url, json={"repo_url": "git@github.com:acme/api.git"})
    assert resp.status_code == expect("DIFF-69", django=200, rust=400)
    if is_rust():
        assert resp.json() == {
            "repo_url": [
                "This repository is already in the project's repository list."
            ]
        }


def test_create_with_repos_defaults_software(world):
    client, user, ws, project = world.full_stack()
    entry = _entry(REPO_A)
    created = _post_project(
        client, ws, settings={"repos": [entry]},
    )
    assert created["default_work_type"] == expect(
        "DIFF-69", django="general", rust="software"
    )
    if is_rust():
        assert created["repos"] == [dict(entry, provider="github")]
    else:
        assert "repos" not in created
    plain = _post_project(client, ws)
    assert plain["default_work_type"] == "general"


def test_create_with_bad_repos_is_400(world):
    client, user, ws, project = world.full_stack()
    tag = uuid.uuid4().hex[:8].upper()
    body = {
        "name": "Bad Repos %s" % tag,
        "identifier": "B%s" % tag[:9],
        "settings": {"repos": [{"id": "x"}]},
    }
    resp = client.post(f"/api/workspaces/{ws['slug']}/projects/", json=body)
    assert resp.status_code == expect("DIFF-69", django=201, rust=400)
    if is_rust():
        assert resp.json() == {
            "settings": ["invalid value for repos: entry 0 id must be a UUID"]
        }


def test_compact_list_has_no_repos_key(world):
    client, user, ws, project = world.full_stack()
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/")
    assert resp.status_code == 200, resp.text[:500]
    assert "repos" not in resp.json()[0]
