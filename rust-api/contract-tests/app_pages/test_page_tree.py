"""Contract: nested (child) pages on the app-tier page endpoints (PDASHOSS01-270).

The default list stays top-level only (the old web app is unchanged);
``?parent=<id>`` lists one page's children, a ``?search=`` spans every
depth, every list/detail row carries ``parent_id`` and ``sub_pages_count``,
detail/create work for child pages, and create/PATCH validate ``parent``
with the ``/api/v1/`` rules (visible in this project, not archived, not the
page itself or one of its descendants).
"""

import pytest

from .conftest import page_url

pytestmark = pytest.mark.contract


def _page(seeder, world, *, name, parent_id=None, owner_id=None, access=0, archived=False):
    page = seeder.create_page(
        world["workspace"]["id"],
        owner_id or world["owner"]["id"],
        name=name,
        parent_id=parent_id,
        access=access,
        archived=archived,
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], page["id"])
    return page


def _by_name(rows):
    return {row["name"]: row for row in rows}


def test_default_list_counts_children_and_hides_them(user_client, world, seeder):
    root = world["page"]
    child = _page(seeder, world, name="Child", parent_id=root["id"])
    _page(seeder, world, name="Grandchild", parent_id=child["id"])
    _page(seeder, world, name="Archived child", parent_id=root["id"], archived=True)
    rows = user_client.get(page_url(world)).json()
    assert [row["id"] for row in rows] == [root["id"]]
    assert rows[0]["parent_id"] is None
    assert rows[0]["sub_pages_count"] == 1


def test_parent_param_lists_children(user_client, world, seeder):
    root = world["page"]
    child = _page(seeder, world, name="Child", parent_id=root["id"])
    _page(seeder, world, name="Grandchild", parent_id=child["id"])
    response = user_client.get(page_url(world), params={"parent": root["id"]})
    assert response.status_code == 200
    rows = response.json()
    assert [row["id"] for row in rows] == [child["id"]]
    assert rows[0]["parent"] == root["id"]
    assert rows[0]["parent_id"] == root["id"]
    assert rows[0]["sub_pages_count"] == 1


def test_parent_param_accepts_hex_without_dashes(user_client, world, seeder):
    child = _page(seeder, world, name="Child", parent_id=world["page"]["id"])
    hex_id = world["page"]["id"].replace("-", "").upper()
    rows = user_client.get(page_url(world), params={"parent": hex_id}).json()
    assert [row["id"] for row in rows] == [child["id"]]


def test_malformed_parent_param_is_400(user_client, world):
    response = user_client.get(page_url(world), params={"parent": "not-a-uuid"})
    assert response.status_code == 400
    assert response.json() == {"error": "Invalid parent page id"}


def test_empty_parent_param_is_the_default_list(user_client, world, seeder):
    _page(seeder, world, name="Child", parent_id=world["page"]["id"])
    rows = user_client.get(page_url(world), params={"parent": ""}).json()
    assert [row["id"] for row in rows] == [world["page"]["id"]]


def test_search_spans_every_depth(user_client, world, seeder):
    child = _page(seeder, world, name="Contract child", parent_id=world["page"]["id"])
    _page(seeder, world, name="Unrelated", parent_id=world["page"]["id"])
    rows = user_client.get(page_url(world), params={"search": "contract"}).json()
    assert {row["id"] for row in rows} == {world["page"]["id"], child["id"]}


def test_archived_page_counts_archived_children(user_client, world, seeder):
    old = _page(seeder, world, name="Old", archived=True)
    _page(seeder, world, name="Old child", parent_id=old["id"], archived=True)
    rows = _by_name(user_client.get(page_url(world)).json())
    assert rows["Old"]["sub_pages_count"] == 1


def test_someone_elses_private_child_is_invisible(user_client, world, seeder, member_user):
    root = world["page"]
    _page(seeder, world, name="Private", parent_id=root["id"], owner_id=member_user["id"], access=1)
    public = _page(seeder, world, name="Public", parent_id=root["id"], owner_id=member_user["id"])
    top = user_client.get(page_url(world)).json()
    assert top[0]["sub_pages_count"] == 1
    children = user_client.get(page_url(world), params={"parent": root["id"]}).json()
    assert [row["id"] for row in children] == [public["id"]]


def test_guest_counts_only_own_children(guest_client, world, seeder, guest_user, db):
    db.execute("UPDATE pages SET owned_by_id=%s WHERE id=%s", (guest_user["id"], world["page"]["id"]))
    _page(seeder, world, name="Guest child", parent_id=world["page"]["id"], owner_id=guest_user["id"])
    _page(seeder, world, name="Owner child", parent_id=world["page"]["id"])
    rows = guest_client.get(page_url(world)).json()
    assert [row["id"] for row in rows] == [world["page"]["id"]]
    assert rows[0]["sub_pages_count"] == 1


def test_child_detail(user_client, world, seeder):
    child = _page(seeder, world, name="Child", parent_id=world["page"]["id"])
    response = user_client.get(page_url(world, child["id"]), params={"track_visit": "false"})
    assert response.status_code == 200
    body = response.json()
    assert body["id"] == child["id"]
    assert body["parent"] == world["page"]["id"]
    assert body["parent_id"] == world["page"]["id"]
    assert body["sub_pages_count"] == 0
    assert body["issue_ids"] == []


def test_parent_detail_counts_children(user_client, world, seeder):
    _page(seeder, world, name="Child", parent_id=world["page"]["id"])
    body = user_client.get(page_url(world, world["page"]["id"]), params={"track_visit": "false"}).json()
    assert body["sub_pages_count"] == 1


def test_create_child_page(user_client, world, db):
    response = user_client.post(page_url(world), json={"name": "New child", "parent": world["page"]["id"]})
    assert response.status_code == 201
    body = response.json()
    assert body["parent"] == world["page"]["id"]
    assert body["parent_id"] == world["page"]["id"]
    assert body["sub_pages_count"] == 0
    assert db.fetchval("SELECT parent_id::text FROM pages WHERE id=%s", (body["id"],)) == world["page"]["id"]


def test_create_under_archived_parent_is_rejected(user_client, world, seeder, db):
    archived = _page(seeder, world, name="Archived", archived=True)
    response = user_client.post(page_url(world), json={"name": "Orphan", "parent": archived["id"]})
    assert response.status_code == 400
    assert response.json() == {"error": "Parent page is archived"}
    assert db.fetchval("SELECT count(*) FROM pages WHERE parent_id=%s", (archived["id"],)) == 0


def test_create_under_invisible_parent_is_rejected(user_client, world, seeder, member_user):
    private = _page(seeder, world, name="Theirs", owner_id=member_user["id"], access=1)
    response = user_client.post(page_url(world), json={"name": "Orphan", "parent": private["id"]})
    assert response.status_code == 400
    assert response.json() == {"error": "Parent page not found in this project"}


def test_create_under_other_project_page_is_rejected(user_client, world, seeder):
    other_project = seeder.create_project(world["workspace"]["id"])
    seeder.create_project_member(world["workspace"]["id"], other_project["id"], world["owner"]["id"], role=20)
    elsewhere = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Elsewhere")
    seeder.link_page_project(world["workspace"]["id"], other_project["id"], elsewhere["id"])
    response = user_client.post(page_url(world), json={"name": "Orphan", "parent": elsewhere["id"]})
    assert response.status_code == 400
    assert response.json() == {"error": "Parent page not found in this project"}


def test_patch_moves_page_and_reports_parent_id(user_client, world, seeder, db):
    page = _page(seeder, world, name="Mover")
    response = user_client.patch(page_url(world, page["id"]), json={"parent": world["page"]["id"]})
    assert response.status_code == 200
    body = response.json()
    assert body["parent"] == world["page"]["id"]
    assert body["parent_id"] == world["page"]["id"]
    assert "sub_pages_count" not in body
    assert db.fetchval("SELECT parent_id::text FROM pages WHERE id=%s", (page["id"],)) == world["page"]["id"]


def test_patch_null_parent_moves_to_top_level(user_client, world, seeder, db):
    child = _page(seeder, world, name="Child", parent_id=world["page"]["id"])
    response = user_client.patch(page_url(world, child["id"]), json={"parent": None})
    assert response.status_code == 200
    assert response.json()["parent_id"] is None
    assert db.fetchval("SELECT parent_id FROM pages WHERE id=%s", (child["id"],)) is None


def test_patch_under_own_descendant_is_rejected(user_client, world, seeder, db):
    child = _page(seeder, world, name="Child", parent_id=world["page"]["id"])
    grandchild = _page(seeder, world, name="Grandchild", parent_id=child["id"])
    response = user_client.patch(page_url(world, world["page"]["id"]), json={"parent": grandchild["id"]})
    assert response.status_code == 400
    assert response.json() == {"error": "A page cannot be nested under itself"}
    assert db.fetchval("SELECT parent_id FROM pages WHERE id=%s", (world["page"]["id"],)) is None


def test_patch_under_itself_is_rejected(user_client, world):
    response = user_client.patch(page_url(world, world["page"]["id"]), json={"parent": world["page"]["id"]})
    assert response.status_code == 400
    assert response.json() == {"error": "A page cannot be nested under itself"}


def test_patch_under_archived_page_is_rejected(user_client, world, seeder):
    archived = _page(seeder, world, name="Archived", archived=True)
    response = user_client.patch(page_url(world, world["page"]["id"]), json={"parent": archived["id"]})
    assert response.status_code == 400
    assert response.json() == {"error": "Parent page is archived"}


def test_patch_under_unlinked_page_is_rejected(user_client, world, seeder):
    # Before PDASHOSS01-270 this surfaced the owner-access error body.
    other_project = seeder.create_project(world["workspace"]["id"])
    seeder.create_project_member(world["workspace"]["id"], other_project["id"], world["owner"]["id"], role=20)
    elsewhere = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Elsewhere")
    seeder.link_page_project(world["workspace"]["id"], other_project["id"], elsewhere["id"])
    response = user_client.patch(page_url(world, world["page"]["id"]), json={"parent": elsewhere["id"]})
    assert response.status_code == 400
    assert response.json() == {"error": "Parent page not found in this project"}
