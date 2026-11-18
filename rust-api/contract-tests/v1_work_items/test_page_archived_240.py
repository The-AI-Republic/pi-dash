"""Contract: archived-project pages via v1 (DIFF-48/49/50, NEWPIDASH-240).

CLI routes (``pidash page list/get/create/update/archive``) ride
``/api/v1/.../pages/``. Old Django returns [] for an archived list
and 404 on detail reads; Rust returns the pages with status on the
list and 409 ``project_archived`` elsewhere. Move is new in Rust.
"""

import httpx
import pytest

from _harness.diffs import expect

pytestmark = pytest.mark.contract

ARCHIVED_AT = "2026-11-13T18:57:37+00:00"

PAGE_LIST_KEYS = {
    "id",
    "name",
    "parent",
    "owned_by",
    "access",
    "is_locked",
    "archived_at",
    "created_at",
    "updated_at",
}

PAGE_DETAIL_KEYS = PAGE_LIST_KEYS | {
    "description_html",
    "description_stripped",
    "description_markdown",
}


@pytest.fixture()
def archived_world(seeder, db, settings):
    owner = seeder.create_user()
    workspace = seeder.create_workspace(owner["id"])
    seeder.create_workspace_member(workspace["id"], owner["id"], role=20)
    project = seeder.create_project(workspace["id"])
    seeder.create_project_member(workspace["id"], project["id"], owner["id"], role=20)
    token = seeder.create_api_token(owner["id"], workspace["id"])
    headers = {"X-Api-Key": token["token"]}
    page = seeder.create_page(workspace["id"], owner["id"], name="Wiki home")
    seeder.link_page_to_project(page["id"], project["id"], workspace["id"])
    world = {
        "owner": owner,
        "workspace": workspace,
        "project": project,
        "token": token,
        "headers": headers,
        "page": page,
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
def active_world(seeder, settings):
    owner = seeder.create_user()
    workspace = seeder.create_workspace(owner["id"])
    seeder.create_workspace_member(workspace["id"], owner["id"], role=20)
    project = seeder.create_project(workspace["id"])
    seeder.create_project_member(workspace["id"], project["id"], owner["id"], role=20)
    token = seeder.create_api_token(owner["id"], workspace["id"])
    headers = {"X-Api-Key": token["token"]}
    page = seeder.create_page(workspace["id"], owner["id"], name="Wiki home")
    seeder.link_page_to_project(page["id"], project["id"], workspace["id"])
    with httpx.Client(base_url=settings.base_url, timeout=30, headers=headers) as client:
        yield client, {
            "owner": owner,
            "workspace": workspace,
            "project": project,
            "token": token,
            "headers": headers,
            "page": page,
        }


def _base(world):
    return f"/api/v1/workspaces/{world['workspace']['slug']}/projects/{world['project']['id']}/pages"


def _assert_archived_body(response, world):
    body = response.json()
    assert set(body) == {"error", "code", "project", "project_identifier", "archived_at"}, body
    assert body["code"] == "project_archived"
    assert body["project"] == world["project"]["identifier"]
    assert body["archived_at"] == "2026-11-13T18:57:37Z"


def test_list_archived_returns_pages_with_status(archived_world):
    api, world = archived_world
    response = api.get(_base(world) + "/")
    assert response.status_code == expect("DIFF-48", django=200, rust=200)
    body = response.json()
    from _harness.diffs import is_rust as _is_rust

    if _is_rust():
        assert body["total_results"] == 1, body
        row = body["results"][0]
        assert set(row) == PAGE_LIST_KEYS | {"project_status", "project_archived_at"}, sorted(row)
        assert row["project_status"] == "archived"
        assert row["project_archived_at"] == "2026-11-13T18:57:37Z"
    else:
        # Django: empty list (old behaviour).
        assert body["total_results"] == 0, body


def test_list_active_gains_only_project_status(active_world):
    api, world = active_world
    response = api.get(_base(world) + "/")
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["total_results"] == 1
    row = body["results"][0]
    from _harness.diffs import is_rust as _is_rust

    if _is_rust():
        assert set(row) == PAGE_LIST_KEYS | {"project_status", "project_archived_at"}, sorted(row)
        assert row["project_status"] == "active"
        assert row["project_archived_at"] is None
    else:
        assert set(row) == PAGE_LIST_KEYS, sorted(row)


def test_get_archived_409(archived_world):
    api, world = archived_world
    response = api.get(f"{_base(world)}/{world['page']['id']}/")
    assert response.status_code == expect("DIFF-49", django=404, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, world)


def test_update_archived_409(archived_world):
    api, world = archived_world
    response = api.patch(f"{_base(world)}/{world['page']['id']}/", json={"name": "Nope"})
    assert response.status_code == expect("DIFF-49", django=404, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, world)


def test_create_archived_409(archived_world):
    api, world = archived_world
    response = api.post(_base(world) + "/", json={"name": "Nope"})
    assert response.status_code == expect("DIFF-49", django=409, rust=409)
    if response.status_code == 409:
        # Django's old body is the page-archive body; Rust is the shared envelope.
        if response.status_code == 409:
            body = response.json()
            # Either envelope counts as 409; the shared envelope has `code`.
            if "code" in body:
                _assert_archived_body(response, world)
            else:
                assert "error" in body


def test_archive_archived_project_409(archived_world):
    api, world = archived_world
    response = api.post(f"{_base(world)}/{world['page']['id']}/archive/")
    assert response.status_code == expect("DIFF-49", django=404, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, world)


def test_move_archived_to_active_succeeds(archived_world, seeder, db, settings):
    api, world = archived_world
    # Second active project in same workspace.
    second = seeder.create_project(world["workspace"]["id"])
    seeder.create_project_member(world["workspace"]["id"], second["id"], world["owner"]["id"], role=20)
    move_url = f"{_base(world)}/{world['page']['id']}/move/"
    response = api.post(move_url, json={"target_project_id": second["id"]})
    from _harness.diffs import is_rust as _is_rust

    if _is_rust():
        assert response.status_code == 200, response.text
        # Reads normally in the target.
        target_base = f"/api/v1/workspaces/{world['workspace']['slug']}/projects/{second['id']}/pages/{world['page']['id']}/"
        detail = api.get(target_base)
        assert detail.status_code == 200, detail.text
        assert detail.json()["name"] == "Wiki home"
        # Source list no longer contains it; target list does with active status.
        src = api.get(_base(world) + "/")
        assert src.json()["total_results"] == 0
        tgt = api.get(f"/api/v1/workspaces/{world['workspace']['slug']}/projects/{second['id']}/pages/")
        assert tgt.json()["total_results"] == 1
        assert tgt.json()["results"][0]["project_status"] == "active"
    else:
        assert response.status_code in (404, 405)


def test_move_to_archived_refused(active_world, seeder, db):
    api, world = active_world
    # Archived target in same workspace.
    target = seeder.create_project(world["workspace"]["id"])
    seeder.create_project_member(world["workspace"]["id"], target["id"], world["owner"]["id"], role=20)
    db.execute("UPDATE projects SET archived_at = %s WHERE id = %s", (ARCHIVED_AT, target["id"]))
    move_url = f"{_base(world)}/{world['page']['id']}/move/"
    response = api.post(move_url, json={"target_project_id": target["id"]})
    from _harness.diffs import is_rust as _is_rust

    if _is_rust():
        assert response.status_code == 409, response.text
        assert response.json()["code"] == "project_archived"
    else:
        assert response.status_code in (404, 405)
    db.execute("UPDATE projects SET archived_at = NULL WHERE id = %s", (target["id"],))


def test_move_cross_workspace_refused(active_world, seeder, db, settings):
    api, world = active_world
    # Second workspace/project.
    owner2 = seeder.create_user()
    ws2 = seeder.create_workspace(owner2["id"])
    seeder.create_workspace_member(ws2["id"], owner2["id"], role=20)
    proj2 = seeder.create_project(ws2["id"])
    seeder.create_project_member(ws2["id"], proj2["id"], world["owner"]["id"], role=20)
    # Ensure the caller is a member of the target workspace? No — cross-workspace must refuse even with membership.
    # We grant membership in ws2 to the caller to prove the workspace check fires, not the gate.
    # Actually the move's target gate will 403 if the caller is not a member of ws2; to prove the
    # cross-workspace 400, the caller must be a member of both. Grant it.
    seeder.create_workspace_member(ws2["id"], world["owner"]["id"], role=20)
    move_url = f"{_base(world)}/{world['page']['id']}/move/"
    response = api.post(move_url, json={"target_project_id": proj2["id"]})
    from _harness.diffs import is_rust as _is_rust

    if _is_rust():
        # Either 400 (workspace) or 403 (gate) depending on slug rewrite: the target
        # identifier is resolved via the source slug, so a UUID in another workspace
        # parses but the workspace check fires. Allow 400.
        assert response.status_code == 400, response.text
    else:
        assert response.status_code in (404, 405)


def test_move_new_project_id_alias(archived_world, seeder):
    api, world = archived_world
    second = seeder.create_project(world["workspace"]["id"])
    seeder.create_project_member(world["workspace"]["id"], second["id"], world["owner"]["id"], role=20)
    move_url = f"{_base(world)}/{world['page']['id']}/move/"
    response = api.post(move_url, json={"new_project_id": second["id"]})
    from _harness.diffs import is_rust as _is_rust
    if _is_rust():
        assert response.status_code == 200, response.text
        body = response.json()
        assert body["project_status"] == "active", body
        assert body["project_archived_at"] is None, body
    else:
        assert response.status_code in (404, 405)


def test_move_self_is_noop(active_world):
    api, world = active_world
    move_url = f"{_base(world)}/{world['page']['id']}/move/"
    response = api.post(move_url, json={"target_project_id": world["project"]["id"]})
    from _harness.diffs import is_rust as _is_rust
    if _is_rust():
        # Self-move is a no-op 200: the page stays linked and readable.
        assert response.status_code == 200, response.text
        assert response.json()["project_status"] == "active"
        assert response.json()["project_archived_at"] is None
        detail = api.get(f"/api/v1/workspaces/{world['workspace']['slug']}/projects/{world['project']['id']}/pages/{world['page']['id']}/")
        assert detail.status_code == 200, detail.text
        src = api.get(_base(world) + "/")
        assert src.json()["total_results"] == 1
    else:
        assert response.status_code in (404, 405)


def test_move_self_archived_refused(archived_world):
    api, world = archived_world
    move_url = f"{_base(world)}/{world['page']['id']}/move/"
    response = api.post(move_url, json={"target_project_id": world["project"]["id"]})
    from _harness.diffs import is_rust as _is_rust
    if _is_rust():
        # Target is archived (source), so the move is refused with 409.
        assert response.status_code == 409, response.text
        assert response.json()["code"] == "project_archived"
    else:
        assert response.status_code in (404, 405)


def test_move_ordering_403_before_409(active_world, seeder, db):
    api, world = active_world
    # Archived target where the caller is NOT a member: gate 403 must win
    # over the archived-target 409.
    target = seeder.create_project(world["workspace"]["id"])
    db.execute("UPDATE projects SET archived_at = %s WHERE id = %s", (ARCHIVED_AT, target["id"]))
    move_url = f"{_base(world)}/{world['page']['id']}/move/"
    response = api.post(move_url, json={"target_project_id": target["id"]})
    from _harness.diffs import is_rust as _is_rust
    if _is_rust():
        assert response.status_code == 403, response.text
    else:
        assert response.status_code in (404, 405)
    db.execute("UPDATE projects SET archived_at = NULL WHERE id = %s", (target["id"],))


def test_move_private_page_forbidden(active_world, seeder):
    api, world = active_world
    # Private page owned by someone else, linked to the source: a member
    # who cannot view it must not be able to move it.
    other = seeder.create_user()
    seeder.create_workspace_member(world["workspace"]["id"], other["id"], role=20)
    private_page = seeder.create_page(world["workspace"]["id"], other["id"], name="Secret", access=1)
    seeder.link_page_to_project(private_page["id"], world["project"]["id"], world["workspace"]["id"])
    second = seeder.create_project(world["workspace"]["id"])
    seeder.create_project_member(world["workspace"]["id"], second["id"], world["owner"]["id"], role=20)
    move_url = f"{_base(world)}/{private_page['id']}/move/"
    response = api.post(move_url, json={"target_project_id": second["id"]})
    from _harness.diffs import is_rust as _is_rust
    if _is_rust():
        assert response.status_code == 403, response.text
    else:
        assert response.status_code in (404, 405)


def test_move_active_to_active_succeeds(active_world, seeder):
    api, world = active_world
    second = seeder.create_project(world["workspace"]["id"])
    seeder.create_project_member(world["workspace"]["id"], second["id"], world["owner"]["id"], role=20)
    move_url = f"{_base(world)}/{world['page']['id']}/move/"
    response = api.post(move_url, json={"target_project_id": second["id"]})
    from _harness.diffs import is_rust as _is_rust
    if _is_rust():
        assert response.status_code == 200, response.text
        assert response.json()["project_status"] == "active"
        assert response.json()["project_archived_at"] is None
        detail = api.get(f"/api/v1/workspaces/{world['workspace']['slug']}/projects/{second['id']}/pages/{world['page']['id']}/")
        assert detail.status_code == 200, detail.text
        src = api.get(_base(world) + "/")
        assert src.json()["total_results"] == 0
    else:
        assert response.status_code in (404, 405)


def test_move_unknown_page_404(active_world, seeder):
    api, world = active_world
    second = seeder.create_project(world["workspace"]["id"])
    seeder.create_project_member(world["workspace"]["id"], second["id"], world["owner"]["id"], role=20)
    import uuid as _uuid
    move_url = f"{_base(world)}/{_uuid.uuid4()}/move/"
    response = api.post(move_url, json={"target_project_id": second["id"]})
    from _harness.diffs import is_rust as _is_rust
    if _is_rust():
        assert response.status_code == 404, response.text
    else:
        assert response.status_code in (404, 405)


def test_move_unknown_target_404(active_world):
    api, world = active_world
    import uuid as _uuid
    move_url = f"{_base(world)}/{world['page']['id']}/move/"
    response = api.post(move_url, json={"target_project_id": str(_uuid.uuid4())})
    from _harness.diffs import is_rust as _is_rust
    if _is_rust():
        assert response.status_code == 404, response.text
    else:
        assert response.status_code in (404, 405)
