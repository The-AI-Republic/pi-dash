"""Contract: project_archived 409 on archived projects (DIFF-45, NEWPIDASH-236).

Old Django hides archived projects inconsistently (200 empty list, 404
detail/by-identifier, 201 create, 200 patch/comments). Rust answers 409
`project_archived` with identifier + archived_at on every project-scoped
issue route after auth and the role gate. Workspace-wide search stays
silent on both.

Issues are seeded via SQL so the tests run against either backend
without depending on the backend's create path.
"""

import httpx
import pytest

from _harness.diffs import expect

pytestmark = pytest.mark.contract

ARCHIVED_AT = "2026-11-13T18:57:37+00:00"


@pytest.fixture()
def archived_world(seeder, db, settings):
    owner = seeder.create_user()
    workspace = seeder.create_workspace(owner["id"])
    seeder.create_workspace_member(workspace["id"], owner["id"], role=20)
    project = seeder.create_project(workspace["id"])
    seeder.create_project_member(workspace["id"], project["id"], owner["id"], role=20)
    token = seeder.create_api_token(owner["id"], workspace["id"])
    issue = seeder.create_issue(workspace["id"], project["id"], name="Contract issue")
    headers = {"X-Api-Key": token["token"]}
    world = {
        "owner": owner,
        "workspace": workspace,
        "project": project,
        "token": token,
        "headers": headers,
        "issue": issue,
    }
    db.execute(
        "UPDATE projects SET archived_at = %s WHERE id = %s",
        (ARCHIVED_AT, world["project"]["id"]),
    )
    with httpx.Client(base_url=settings.base_url, timeout=30, headers=headers) as client:
        yield client, world
    db.execute(
        "UPDATE projects SET archived_at = NULL WHERE id = %s",
        (world["project"]["id"],),
    )


@pytest.fixture()
def active_world(seeder, db, settings):
    owner = seeder.create_user()
    workspace = seeder.create_workspace(owner["id"])
    seeder.create_workspace_member(workspace["id"], owner["id"], role=20)
    project = seeder.create_project(workspace["id"])
    seeder.create_project_member(workspace["id"], project["id"], owner["id"], role=20)
    token = seeder.create_api_token(owner["id"], workspace["id"])
    issue = seeder.create_issue(workspace["id"], project["id"], name="Contract issue")
    headers = {"X-Api-Key": token["token"]}
    with httpx.Client(base_url=settings.base_url, timeout=30, headers=headers) as client:
        yield client, {
            "owner": owner,
            "workspace": workspace,
            "project": project,
            "token": token,
            "headers": headers,
            "issue": issue,
        }


def _base(world):
    return f"/api/v1/workspaces/{world['workspace']['slug']}/projects/{world['project']['id']}/work-items"


def _assert_archived_body(response, world):
    body = response.json()
    assert set(body) == {
        "error",
        "code",
        "project",
        "project_identifier",
        "archived_at",
    }, body
    assert body["code"] == "project_archived"
    assert body["project"] == world["project"]["identifier"]
    assert body["project_identifier"] == world["project"]["identifier"]
    assert body["archived_at"] == "2026-11-13T18:57:37Z"
    assert body["error"] == (
        f"Project {world['project']['identifier']} is archived (since "
        "2026-11-13T18:57:37Z). Unarchive it to read or change its work items."
    )


def test_archived_list_409(archived_world):
    api, world = archived_world
    response = api.get(_base(world) + "/")
    assert response.status_code == expect("DIFF-45", django=200, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, world)


def test_archived_detail_uuid_409(archived_world):
    api, world = archived_world
    response = api.get(f"{_base(world)}/{world['issue']['id']}/")
    assert response.status_code == expect("DIFF-45", django=404, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, world)


def test_archived_detail_by_identifier_409(archived_world):
    api, world = archived_world
    identifier = world["project"]["identifier"]
    # The SQL-seeded issue carries the seeder's default sequence_id 1.
    seq = 1
    response = api.get(
        f"/api/v1/workspaces/{world['workspace']['slug']}/work-items/{identifier}-{seq}/"
    )
    # On Django the by-identifier read misses archived projects (404);
    # on Rust the archived check fires first (409). The identifier is
    # upper-case already (seeder upper-cases).
    assert response.status_code == expect("DIFF-45", django=404, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, world)


def test_archived_create_409(archived_world):
    api, world = archived_world
    response = api.post(_base(world) + "/", json={"name": "Nope"})
    assert response.status_code == expect("DIFF-45", django=201, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, world)


def test_archived_patch_409(archived_world):
    api, world = archived_world
    response = api.patch(
        f"{_base(world)}/{world['issue']['id']}/", json={"name": "Nope"}
    )
    assert response.status_code == expect("DIFF-45", django=200, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, world)


def test_archived_comments_409(archived_world):
    api, world = archived_world
    response = api.get(f"{_base(world)}/{world['issue']['id']}/comments/")
    assert response.status_code == expect("DIFF-45", django=200, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, world)


def test_active_list_unchanged(active_world):
    api, world = active_world
    response = api.get(_base(world) + "/")
    assert response.status_code == 200, response.text


