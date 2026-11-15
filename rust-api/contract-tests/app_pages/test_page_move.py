"""Contract: app page move (PDASHOSS01-347).

``POST /api/workspaces/<slug>/projects/<project_id>/pages/<page_id>/move/``
with ``{"project_id": <target>}`` (``app/views/page/base.py``
``PageMoveEndpoint`` over ``utils/page_move.py``), the action the web UIs
call. Same rules and refusal bodies as the v1 route
(``v1_work_items/test_page_move.py`` pins those in depth); success answers
204 with an empty body.
"""

import pytest

from _harness.auth import login_session
from _harness.http import api_client

pytestmark = pytest.mark.contract

NOT_FOUND = {"error": "Page not found"}
REQUIRED = {"error": "project_id is required"}
NOT_SOURCE_ADMIN = {"error": "Only a project admin can move a page to another project"}
SAME_PROJECT = {"error": "The page is already in this project"}
NOT_IN_WORKSPACE = {"error": "The target project is not in this workspace"}
TARGET_ARCHIVED = {"error": "The target project is archived"}
NO_TARGET_WRITE = {
    "error": "You need write access to the target project to move a page into it"
}
LOCKED = {"error": "Page is locked", "error_code": 4701, "error_message": "PAGE_LOCKED"}


def move_url(world, page_id, project_id=None):
    project_id = project_id or world["project"]["id"]
    return (
        f"/api/workspaces/{world['workspace']['slug']}/projects/{project_id}"
        f"/pages/{page_id}/move/"
    )


def add_target(seeder, world, *, role=20, user_id=None):
    project = seeder.create_project(world["workspace"]["id"])
    if role is not None:
        seeder.create_project_member(
            world["workspace"]["id"], project["id"], user_id or world["owner"]["id"], role=role
        )
    return project


def live_links(db, page_id):
    rows = db.fetchall(
        "SELECT project_id FROM project_pages WHERE page_id=%s AND deleted_at IS NULL",
        (page_id,),
    )
    return sorted(str(row["project_id"]) for row in rows)


def logged_in(settings, user):
    client = api_client(settings.base_url)
    login_session(client, email=user["email"], password=user["password"])
    return client


def test_move_relinks_page_and_sub_pages(user_client, world, seeder, db):
    target = add_target(seeder, world)
    root = world["page"]
    child = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Child", parent_id=root["id"]
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], child["id"])
    favorite = seeder.create_favorite(
        world["workspace"]["id"], world["project"]["id"], world["owner"]["id"], root["id"]
    )
    before = db.fetchone(
        "SELECT name, owned_by_id, description_html FROM pages WHERE id=%s", (root["id"],)
    )

    response = user_client.post(move_url(world, root["id"]), json={"project_id": target["id"]})
    assert response.status_code == 204, response.text
    assert response.content == b""

    for page in (root, child):
        assert live_links(db, page["id"]) == [target["id"]]
        assert str(
            db.fetchval("SELECT moved_to_project FROM pages WHERE id=%s", (page["id"],))
        ) == target["id"]
    assert str(db.fetchval("SELECT parent_id FROM pages WHERE id=%s", (child["id"],))) == root["id"]
    after = db.fetchone(
        "SELECT name, owned_by_id, description_html FROM pages WHERE id=%s", (root["id"],)
    )
    assert after == before
    assert (
        db.fetchval("SELECT deleted_at FROM user_favorites WHERE id=%s", (favorite["id"],))
        is not None
    )
    log = db.fetchone(
        "SELECT entity_identifier, entity_type, created_by_id FROM page_logs"
        " WHERE page_id=%s AND entity_name='moved_to_project'",
        (root["id"],),
    )
    assert str(log["entity_identifier"]) == target["id"]
    assert log["entity_type"] == "project"
    assert str(log["created_by_id"]) == world["owner"]["id"]

    # The page now reads under the target and 404s under the source.
    detail = f"/api/workspaces/{world['workspace']['slug']}/projects/{{}}/pages/{root['id']}/"
    assert user_client.get(detail.format(target["id"])).status_code == 200
    assert user_client.get(detail.format(world["project"]["id"])).status_code == 404


def test_child_moved_alone_loses_parent(user_client, world, seeder, db):
    target = add_target(seeder, world)
    child = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Child", parent_id=world["page"]["id"]
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], child["id"])
    response = user_client.post(move_url(world, child["id"]), json={"project_id": target["id"]})
    assert response.status_code == 204, response.text
    assert db.fetchval("SELECT parent_id FROM pages WHERE id=%s", (child["id"],)) is None
    assert live_links(db, world["page"]["id"]) == [world["project"]["id"]]


@pytest.mark.parametrize("payload", [{}, {"project_id": None}, {"project_id": ""}])
def test_project_id_is_required(user_client, world, payload):
    response = user_client.post(move_url(world, world["page"]["id"]), json=payload)
    assert response.status_code == 400
    assert response.json() == REQUIRED


def test_same_project_is_refused(user_client, world):
    response = user_client.post(
        move_url(world, world["page"]["id"]), json={"project_id": world["project"]["id"]}
    )
    assert response.status_code == 400
    assert response.json() == SAME_PROJECT


def test_target_in_other_workspace_is_refused(user_client, world, world2, db):
    response = user_client.post(
        move_url(world, world["page"]["id"]), json={"project_id": world2["project"]["id"]}
    )
    assert response.status_code == 400
    assert response.json() == NOT_IN_WORKSPACE
    assert live_links(db, world["page"]["id"]) == [world["project"]["id"]]


def test_archived_target_is_refused(user_client, world, seeder, db):
    target = add_target(seeder, world)
    db.execute("UPDATE projects SET archived_at=now() WHERE id=%s", (target["id"],))
    response = user_client.post(
        move_url(world, world["page"]["id"]), json={"project_id": target["id"]}
    )
    assert response.status_code == 400
    assert response.json() == TARGET_ARCHIVED


def test_locked_page_is_refused(user_client, world, seeder, db):
    target = add_target(seeder, world)
    db.execute("UPDATE pages SET is_locked=true WHERE id=%s", (world["page"]["id"],))
    response = user_client.post(
        move_url(world, world["page"]["id"]), json={"project_id": target["id"]}
    )
    assert response.status_code == 400
    assert response.json() == LOCKED
    assert live_links(db, world["page"]["id"]) == [world["project"]["id"]]


def test_member_of_source_is_refused(settings, member_user, world, seeder):
    target = add_target(seeder, world, user_id=member_user["id"])
    client = logged_in(settings, member_user)
    try:
        response = client.post(
            move_url(world, world["page"]["id"]), json={"project_id": target["id"]}
        )
    finally:
        client.close()
    assert response.status_code == 403
    assert response.json() == NOT_SOURCE_ADMIN


@pytest.mark.parametrize("target_role", [None, 5])
def test_no_write_access_in_target_is_refused(user_client, world, seeder, target_role):
    target = add_target(seeder, world, role=target_role)
    response = user_client.post(
        move_url(world, world["page"]["id"]), json={"project_id": target["id"]}
    )
    assert response.status_code == 403
    assert response.json() == NO_TARGET_WRITE


def test_guest_of_source_is_denied(guest_client, world, seeder):
    target = add_target(seeder, world)
    response = guest_client.post(
        move_url(world, world["page"]["id"]), json={"project_id": target["id"]}
    )
    assert response.status_code == 403


def test_unknown_page_is_not_found(user_client, world, seeder):
    target = add_target(seeder, world)
    response = user_client.post(
        move_url(world, "00000000-0000-4000-8000-000000000000"), json={"project_id": target["id"]}
    )
    assert response.status_code == 404
    assert response.json() == NOT_FOUND


def test_anonymous_is_refused(anon_client, world):
    response = anon_client.post(move_url(world, world["page"]["id"]), json={"project_id": "x"})
    assert response.status_code in (401, 403)
