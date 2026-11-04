"""Contract: v1 page move (PDASHOSS01-347).

``POST /api/v1/workspaces/<slug>/projects/<project_id>/pages/<page_id>/move/``
with ``{"project_id": <target>}`` (``api/views/page.py`` ``PageMoveAPIEndpoint``
over ``utils/page_move.py``). Success answers 200 with the page detail as
``GET .../projects/<target>/pages/<page_id>/`` renders it; the page then 404s
under the source. Refusals answer view-inline ``{"error": ...}`` bodies in a
fixed check order; nothing is written on a refusal.
"""

import httpx
import pytest

pytestmark = pytest.mark.contract

NOT_FOUND = {"error": "Page not found"}
RESOURCE_MISSING = {"error": "The requested resource does not exist."}
CLASS_DENIAL = {"detail": "You do not have permission to perform this action."}
REQUIRED = {"error": "project_id is required"}
INVALID = {"error": "project_id must be a valid project id"}
NOT_SOURCE_ADMIN = {"error": "Only a project admin can move a page to another project"}
SAME_PROJECT = {"error": "The page is already in this project"}
NOT_IN_WORKSPACE = {"error": "The target project is not in this workspace"}
TARGET_ARCHIVED = {"error": "The target project is archived"}
NO_TARGET_WRITE = {
    "error": "You need write access to the target project to move a page into it"
}
LOCKED = {"error": "Page is locked", "error_code": 4701, "error_message": "PAGE_LOCKED"}


def pages_url(world, project_id, page_id=None, action=None):
    url = f"/api/v1/workspaces/{world['workspace']['slug']}/projects/{project_id}/pages/"
    if page_id is not None:
        url += f"{page_id}/"
    if action is not None:
        url += f"{action}/"
    return url


def move_url(world, page_id, project_id=None):
    return pages_url(world, project_id or world["project"]["id"], page_id, "move")


def add_project(seeder, world, *, role=20, user_id=None):
    """A second project in the world's workspace with ``user`` as ``role``."""
    project = seeder.create_project(world["workspace"]["id"])
    if role is not None:
        seeder.create_project_member(
            world["workspace"]["id"], project["id"], user_id or world["owner"]["id"], role=role
        )
    return project


def add_page(seeder, world, *, project_id=None, **kwargs):
    page = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], **kwargs)
    seeder.link_page_to_project(
        page["id"], project_id or world["project"]["id"], world["workspace"]["id"]
    )
    return page


def live_links(db, page_id):
    rows = db.fetchall(
        "SELECT project_id FROM project_pages WHERE page_id=%s AND deleted_at IS NULL",
        (page_id,),
    )
    return sorted(str(row["project_id"]) for row in rows)


def client_for(settings, seeder, world, *, source_role, target_project=None, target_role=None):
    """An API client for a fresh user with the given roles."""
    user = seeder.create_user()
    seeder.create_workspace_member(world["workspace"]["id"], user["id"], role=15)
    seeder.create_project_member(
        world["workspace"]["id"], world["project"]["id"], user["id"], role=source_role
    )
    if target_project is not None and target_role is not None:
        seeder.create_project_member(
            world["workspace"]["id"], target_project["id"], user["id"], role=target_role
        )
    token = seeder.create_api_token(user["id"], world["workspace"]["id"])
    return httpx.Client(
        base_url=settings.base_url, timeout=30, headers={"X-Api-Key": token["token"]}
    ), user


# ---------------------------------------------------------------------------
# Success
# ---------------------------------------------------------------------------


def test_move_with_sub_pages(api, world, seeder, db):
    target = add_project(seeder, world)
    root = add_page(seeder, world, name="Root")
    child = add_page(seeder, world, name="Child", parent_id=root["id"])
    grandchild = add_page(seeder, world, name="Grandchild", parent_id=child["id"])
    sibling = add_page(seeder, world, name="Sibling")
    favorite = seeder.create_favorite(
        world["workspace"]["id"], world["project"]["id"], world["owner"]["id"], root["id"]
    )
    child_favorite = seeder.create_favorite(
        world["workspace"]["id"], world["project"]["id"], world["owner"]["id"], child["id"]
    )
    sibling_favorite = seeder.create_favorite(
        world["workspace"]["id"], world["project"]["id"], world["owner"]["id"], sibling["id"]
    )
    before = db.fetchone(
        "SELECT name, owned_by_id, description_html, created_at FROM pages WHERE id=%s",
        (root["id"],),
    )

    response = api.post(move_url(world, root["id"]), json={"project_id": target["id"]})
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["id"] == root["id"]
    assert body["parent"] is None

    in_target = api.get(pages_url(world, target["id"], root["id"]))
    assert in_target.status_code == 200
    assert body == in_target.json()
    gone = api.get(pages_url(world, world["project"]["id"], root["id"]))
    assert gone.status_code == 404
    assert gone.json() == RESOURCE_MISSING

    for page in (root, child, grandchild):
        assert live_links(db, page["id"]) == [target["id"]]
        row = db.fetchone(
            "SELECT moved_to_project, updated_by_id FROM pages WHERE id=%s", (page["id"],)
        )
        assert str(row["moved_to_project"]) == target["id"]
        assert str(row["updated_by_id"]) == world["owner"]["id"]
        link = db.fetchone(
            "SELECT updated_by_id FROM project_pages WHERE page_id=%s AND deleted_at IS NULL",
            (page["id"],),
        )
        assert str(link["updated_by_id"]) == world["owner"]["id"]
    # Children keep their parents; the sibling stays put.
    assert str(db.fetchval("SELECT parent_id FROM pages WHERE id=%s", (child["id"],))) == root["id"]
    assert (
        str(db.fetchval("SELECT parent_id FROM pages WHERE id=%s", (grandchild["id"],)))
        == child["id"]
    )
    assert live_links(db, sibling["id"]) == [world["project"]["id"]]
    assert db.fetchval("SELECT moved_to_project FROM pages WHERE id=%s", (sibling["id"],)) is None
    # Identity and body untouched.
    after = db.fetchone(
        "SELECT name, owned_by_id, description_html, created_at FROM pages WHERE id=%s",
        (root["id"],),
    )
    assert after == before
    # Favourites of the moved pages (source project) are soft-deleted.
    for fav in (favorite, child_favorite):
        assert (
            db.fetchval("SELECT deleted_at FROM user_favorites WHERE id=%s", (fav["id"],))
            is not None
        )
    assert (
        db.fetchval("SELECT deleted_at FROM user_favorites WHERE id=%s", (sibling_favorite["id"],))
        is None
    )
    # One move log row on the root page.
    logs = db.fetchall(
        "SELECT page_id, entity_identifier, entity_name, entity_type, workspace_id,"
        " created_by_id, updated_by_id, deleted_at, transaction FROM page_logs"
        " WHERE entity_name='moved_to_project' AND page_id IN (%s, %s, %s)",
        (root["id"], child["id"], grandchild["id"]),
    )
    assert len(logs) == 1
    log = logs[0]
    assert str(log["page_id"]) == root["id"]
    assert str(log["entity_identifier"]) == target["id"]
    assert log["entity_type"] == "project"
    assert str(log["workspace_id"]) == world["workspace"]["id"]
    assert str(log["created_by_id"]) == world["owner"]["id"]
    assert log["updated_by_id"] is None
    assert log["deleted_at"] is None
    assert log["transaction"] is not None
    # Listed in the target, gone from the source list.
    target_ids = {p["id"] for p in api.get(pages_url(world, target["id"])).json()["results"]}
    source_ids = {
        p["id"] for p in api.get(pages_url(world, world["project"]["id"])).json()["results"]
    }
    assert {root["id"], child["id"], grandchild["id"]} <= target_ids
    assert not ({root["id"], child["id"], grandchild["id"]} & source_ids)
    assert sibling["id"] in source_ids


def test_child_moved_alone_loses_parent(api, world, seeder, db):
    target = add_project(seeder, world)
    parent = add_page(seeder, world, name="Parent")
    child = add_page(seeder, world, name="Child", parent_id=parent["id"])
    grandchild = add_page(seeder, world, name="Grandchild", parent_id=child["id"])
    before = db.fetchone("SELECT updated_at FROM pages WHERE id=%s", (parent["id"],))

    response = api.post(move_url(world, child["id"]), json={"project_id": target["id"]})
    assert response.status_code == 200, response.text
    assert response.json()["parent"] is None
    assert db.fetchval("SELECT parent_id FROM pages WHERE id=%s", (child["id"],)) is None
    assert (
        str(db.fetchval("SELECT parent_id FROM pages WHERE id=%s", (grandchild["id"],)))
        == child["id"]
    )
    assert live_links(db, child["id"]) == [target["id"]]
    assert live_links(db, grandchild["id"]) == [target["id"]]
    # The parent stays in the source, untouched.
    assert live_links(db, parent["id"]) == [world["project"]["id"]]
    row = db.fetchone("SELECT moved_to_project, updated_at FROM pages WHERE id=%s", (parent["id"],))
    assert row["moved_to_project"] is None
    assert row["updated_at"] == before["updated_at"]


def test_move_skips_deleted_and_unlinked_descendants(api, world, seeder, db):
    target = add_project(seeder, world)
    other = add_project(seeder, world)
    root = add_page(seeder, world, name="Root")
    deleted_child = add_page(seeder, world, name="Deleted", parent_id=root["id"])
    db.execute("UPDATE pages SET deleted_at=now() WHERE id=%s", (deleted_child["id"],))
    elsewhere = add_page(seeder, world, project_id=other["id"], name="Elsewhere", parent_id=root["id"])

    response = api.post(move_url(world, root["id"]), json={"project_id": target["id"]})
    assert response.status_code == 200, response.text
    assert live_links(db, deleted_child["id"]) == [world["project"]["id"]]
    assert live_links(db, elsewhere["id"]) == [other["id"]]
    for page in (deleted_child, elsewhere):
        assert db.fetchval("SELECT moved_to_project FROM pages WHERE id=%s", (page["id"],)) is None


def test_page_already_linked_to_target_keeps_one_link(api, world, seeder, db):
    target = add_project(seeder, world)
    page = add_page(seeder, world)
    seeder.link_page_to_project(page["id"], target["id"], world["workspace"]["id"])

    response = api.post(move_url(world, page["id"]), json={"project_id": target["id"]})
    assert response.status_code == 200, response.text
    assert live_links(db, page["id"]) == [target["id"]]
    dropped = db.fetchall(
        "SELECT project_id FROM project_pages WHERE page_id=%s AND deleted_at IS NOT NULL",
        (page["id"],),
    )
    assert [str(row["project_id"]) for row in dropped] == [world["project"]["id"]]


def test_move_by_identifier_and_form_body(api, world, seeder, db):
    target = add_project(seeder, world)
    page = add_page(seeder, world)
    response = api.post(
        pages_url(world, world["project"]["identifier"], page["id"], "move"),
        data={"project_id": target["id"]},
    )
    assert response.status_code == 200, response.text
    assert live_links(db, page["id"]) == [target["id"]]


def test_member_of_target_can_receive(settings, world, seeder, db):
    target = add_project(seeder, world, role=None)
    client, user = client_for(
        settings, seeder, world, source_role=20, target_project=target, target_role=15
    )
    page = add_page(seeder, world)
    with client:
        response = client.post(move_url(world, page["id"]), json={"project_id": target["id"]})
    assert response.status_code == 200, response.text
    assert live_links(db, page["id"]) == [target["id"]]
    log = db.fetchone(
        "SELECT created_by_id FROM page_logs WHERE page_id=%s AND entity_name='moved_to_project'",
        (page["id"],),
    )
    assert str(log["created_by_id"]) == user["id"]


# ---------------------------------------------------------------------------
# Refusals
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    "payload, expected",
    [
        ({}, REQUIRED),
        ({"project_id": None}, REQUIRED),
        ({"project_id": ""}, REQUIRED),
        ({"project_id": "not-a-uuid"}, INVALID),
        ({"project_id": 123}, INVALID),
        ({"project_id": True}, INVALID),
        ({"project_id": ["x"]}, INVALID),
        (["not", "an", "object"], REQUIRED),
    ],
)
def test_project_id_validation(api, world, seeder, db, payload, expected):
    page = add_page(seeder, world)
    response = api.post(move_url(world, page["id"]), json=payload)
    assert response.status_code == 400
    assert response.json() == expected
    assert live_links(db, page["id"]) == [world["project"]["id"]]


def test_empty_body_is_required(api, world, seeder):
    page = add_page(seeder, world)
    response = api.post(move_url(world, page["id"]))
    assert response.status_code == 400
    assert response.json() == REQUIRED


def test_member_of_source_is_refused(settings, world, seeder, db):
    target = add_project(seeder, world, role=None)
    client, _ = client_for(
        settings, seeder, world, source_role=15, target_project=target, target_role=20
    )
    page = add_page(seeder, world)
    with client:
        response = client.post(move_url(world, page["id"]), json={"project_id": target["id"]})
        # Check 1 precedes the admin check.
        invalid = client.post(move_url(world, page["id"]), json={"project_id": "x"})
    assert response.status_code == 403
    assert response.json() == NOT_SOURCE_ADMIN
    assert invalid.status_code == 400
    assert invalid.json() == INVALID
    assert live_links(db, page["id"]) == [world["project"]["id"]]


def test_guest_of_source_is_class_denied(settings, world, seeder):
    target = add_project(seeder, world)
    client, _ = client_for(settings, seeder, world, source_role=5)
    page = add_page(seeder, world)
    with client:
        response = client.post(move_url(world, page["id"]), json={"project_id": target["id"]})
    assert response.status_code == 403
    assert response.json() == CLASS_DENIAL


def test_same_project_is_refused(api, world, seeder):
    page = add_page(seeder, world, locked=True)
    response = api.post(move_url(world, page["id"]), json={"project_id": world["project"]["id"]})
    # Same-project precedes the lock check.
    assert response.status_code == 400
    assert response.json() == SAME_PROJECT


def test_target_in_other_workspace_is_refused(api, world, other_world, seeder, db):
    page = add_page(seeder, world)
    response = api.post(
        move_url(world, page["id"]), json={"project_id": other_world["project"]["id"]}
    )
    assert response.status_code == 400
    assert response.json() == NOT_IN_WORKSPACE
    assert live_links(db, page["id"]) == [world["project"]["id"]]


def test_unknown_and_deleted_targets_are_refused(api, world, seeder, db):
    page = add_page(seeder, world)
    unknown = api.post(
        move_url(world, page["id"]),
        json={"project_id": "00000000-0000-4000-8000-000000000001"},
    )
    assert unknown.status_code == 400
    assert unknown.json() == NOT_IN_WORKSPACE
    deleted = add_project(seeder, world)
    db.execute("UPDATE projects SET deleted_at=now() WHERE id=%s", (deleted["id"],))
    response = api.post(move_url(world, page["id"]), json={"project_id": deleted["id"]})
    assert response.status_code == 400
    assert response.json() == NOT_IN_WORKSPACE


def test_archived_target_is_refused(api, world, seeder, db):
    target = add_project(seeder, world)
    db.execute("UPDATE projects SET archived_at=now() WHERE id=%s", (target["id"],))
    page = add_page(seeder, world, locked=True)
    response = api.post(move_url(world, page["id"]), json={"project_id": target["id"]})
    assert response.status_code == 400
    assert response.json() == TARGET_ARCHIVED
    assert live_links(db, page["id"]) == [world["project"]["id"]]


@pytest.mark.parametrize("target_role", [None, 5])
def test_no_write_access_in_target_is_refused(api, world, seeder, db, target_role):
    target = add_project(seeder, world, role=target_role)
    page = add_page(seeder, world, locked=True)
    response = api.post(move_url(world, page["id"]), json={"project_id": target["id"]})
    # Target write access precedes the lock check.
    assert response.status_code == 403
    assert response.json() == NO_TARGET_WRITE
    assert live_links(db, page["id"]) == [world["project"]["id"]]


def test_locked_page_is_refused(api, world, seeder, db):
    target = add_project(seeder, world)
    page = add_page(seeder, world, locked=True)
    response = api.post(move_url(world, page["id"]), json={"project_id": target["id"]})
    assert response.status_code == 400
    assert response.json() == LOCKED
    assert response.text.startswith('{"error":')
    assert live_links(db, page["id"]) == [world["project"]["id"]]
    assert (
        db.fetchval("SELECT count(*) FROM page_logs WHERE page_id=%s", (page["id"],)) == 0
    )


def test_invisible_and_unknown_pages_are_not_found(api, world, seeder):
    target = add_project(seeder, world)
    stranger = seeder.create_user()
    private = seeder.create_page(world["workspace"]["id"], stranger["id"], access=1)
    seeder.link_page_to_project(private["id"], world["project"]["id"], world["workspace"]["id"])
    for page_id in (private["id"], "00000000-0000-4000-8000-0000000000aa"):
        response = api.post(move_url(world, page_id), json={"project_id": target["id"]})
        assert response.status_code == 404
        assert response.json() == NOT_FOUND
    # The lookup precedes body validation.
    response = api.post(move_url(world, private["id"]), json={})
    assert response.status_code == 404


def test_anonymous_is_unauthorized(anon, world, seeder):
    page = add_page(seeder, world)
    response = anon.post(move_url(world, page["id"]), json={"project_id": world["project"]["id"]})
    assert response.status_code == 401
