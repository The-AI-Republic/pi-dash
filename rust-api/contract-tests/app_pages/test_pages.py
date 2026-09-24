"""Contract: page list/create/retrieve/patch/delete + summary.

Covers ``app/urls/page.py`` for ``PageViewSet``: ``pages-summary/``,
``pages/`` (list/create) and ``pages/<page_id>/`` (retrieve/patch/delete).

Unauthenticated access is denied throughout (``ProjectPagePermission`` —
removing that permission class flips these tripwires red), and every lookup
is scoped to the URL workspace+project (the isolation cases below).
"""

import uuid

import pytest

from .conftest import page_url, pages_base

pytestmark = pytest.mark.contract

# app/serializers/page.py PageSerializer.Meta.fields, minus the write-only
# ``labels`` input.
LIST_ROW_KEYS = {
    "id",
    "name",
    "owned_by",
    "access",
    "color",
    "parent",
    "is_favorite",
    "is_locked",
    "archived_at",
    "workspace",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "view_props",
    "logo_props",
    "label_ids",
    "project_ids",
}

DETAIL_ROW_KEYS = LIST_ROW_KEYS | {"description_html"}

# partial_update answers with the update-bound PageDetailSerializer over a
# plain Page.objects.get() instance, while create re-fetches through the
# annotated get_queryset(). The annotation-backed keys (is_favorite Exists,
# label_ids/project_ids Coalesce) have no attribute on the plain instance,
# and all three fields are required=False, so DRF drops them (SkipField) —
# the PATCH body is always DETAIL minus exactly those three keys.
PATCH_ROW_KEYS = DETAIL_ROW_KEYS - {"is_favorite", "label_ids", "project_ids"}


def test_list_shape(user_client, world):
    response = user_client.get(page_url(world))
    assert response.status_code == 200
    rows = response.json()
    assert isinstance(rows, list) and len(rows) == 1
    row = rows[0]
    assert set(row) == LIST_ROW_KEYS, f"list row keys drifted: {sorted(row)}"
    assert row["id"] == world["page"]["id"]
    assert row["name"] == "Contract page"
    assert row["owned_by"] == world["owner"]["id"]
    assert row["access"] == 0
    assert row["is_favorite"] is False
    assert row["is_locked"] is False
    assert row["archived_at"] is None
    assert row["label_ids"] == []
    assert row["project_ids"] == [world["project"]["id"]]


def test_list_requires_auth(anon_client, world):
    assert anon_client.get(page_url(world)).status_code in (401, 403)


def test_list_denied_for_outsider(outsider_client, world):
    # Authenticated but no project membership: ProjectPagePermission denies.
    assert outsider_client.get(page_url(world)).status_code in (401, 403)


def test_list_excludes_child_pages(user_client, world, seeder):
    child = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Child page", parent_id=world["page"]["id"]
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], child["id"])
    rows = user_client.get(page_url(world)).json()
    assert {row["id"] for row in rows} == {world["page"]["id"]}


def test_list_excludes_unlinked_project(user_client, world, seeder):
    other_project = seeder.create_project(world["workspace"]["id"])
    seeder.create_project_member(
        world["workspace"]["id"], other_project["id"], world["owner"]["id"], role=20
    )
    other = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Other project page")
    seeder.link_page_project(world["workspace"]["id"], other_project["id"], other["id"])
    rows = user_client.get(page_url(world)).json()
    assert {row["id"] for row in rows} == {world["page"]["id"]}


def test_list_guest_sees_only_own_pages(guest_client, world, seeder, db):
    # Seeded projects have guest_view_all_features=false, so guests see only
    # pages they own.
    assert guest_client.get(page_url(world)).json() == []
    own = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Guest page")
    guest_id = db.fetchval(
        "SELECT member_id FROM project_members WHERE project_id=%s AND role=5",
        (world["project"]["id"],),
    )
    db.execute("UPDATE pages SET owned_by_id=%s WHERE id=%s", (guest_id, own["id"]))
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], own["id"])
    rows = guest_client.get(page_url(world)).json()
    assert [row["id"] for row in rows] == [own["id"]]


def test_list_guest_sees_public_page_when_flag_on(guest_client, world, db):
    db.execute("UPDATE projects SET guest_view_all_features=true WHERE id=%s", (world["project"]["id"],))
    rows = guest_client.get(page_url(world)).json()
    assert {row["id"] for row in rows} == {world["page"]["id"]}


def test_create_shape(user_client, world, db):
    response = user_client.post(page_url(world), json={"name": "Created page"})
    assert response.status_code == 201
    body = response.json()
    # Unlike retrieve, create returns the bare detail serializer (no issue_ids).
    assert set(body) == DETAIL_ROW_KEYS, f"create keys drifted: {sorted(body)}"
    assert body["name"] == "Created page"
    assert body["owned_by"] == world["owner"]["id"]
    assert body["description_html"] == "<p></p>"
    assert body["project_ids"] == [world["project"]["id"]]
    row = db.fetchone("SELECT name, owned_by_id FROM pages WHERE id=%s", (body["id"],))
    assert row["name"] == "Created page"


def test_create_denied_for_guest(guest_client, world):
    # Only ADMIN/MEMBER roles may POST (permission tripwire).
    assert guest_client.post(page_url(world), json={"name": "Guest page"}).status_code in (401, 403)


def test_create_requires_auth(anon_client, world):
    assert anon_client.post(page_url(world), json={"name": "Anon page"}).status_code in (401, 403)


def test_retrieve_shape(user_client, world):
    response = user_client.get(page_url(world, world["page"]["id"]))
    assert response.status_code == 200
    body = response.json()
    assert set(body) == DETAIL_ROW_KEYS | {"issue_ids"}, f"retrieve keys drifted: {sorted(body)}"
    assert body["id"] == world["page"]["id"]
    assert body["description_html"] == "<p></p>"
    assert body["issue_ids"] == []


def test_retrieve_unlinked_page_is_404(user_client, world, seeder):
    # The page exists in the workspace (so permission passes) but is not
    # linked to the URL project: the detail queryset finds nothing.
    other_project = seeder.create_project(world["workspace"]["id"])
    seeder.create_project_member(
        world["workspace"]["id"], other_project["id"], world["owner"]["id"], role=20
    )
    other = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Elsewhere")
    seeder.link_page_project(world["workspace"]["id"], other_project["id"], other["id"])
    response = user_client.get(page_url(world, other["id"]))
    assert response.status_code == 404
    assert response.json() == {"error": "Page not found"}


def test_retrieve_unknown_page_id_is_404(user_client, world):
    # NOTE (ported behavior): ProjectPagePermission.has_permission looks the
    # page up with Page.objects.get, so an id that exists nowhere raises
    # DoesNotExist inside the permission check — BaseViewSet.handle_exception
    # maps ObjectDoesNotExist to 404 (never the view's "Page not found"
    # body, which only the project-scoped queryset path returns). Pinned so
    # the Rust port reproduces it instead of "fixing" it.
    response = user_client.get(page_url(world, str(uuid.uuid4())))
    assert response.status_code == 404
    assert response.json() == {"error": "The required object does not exist."}


def test_retrieve_requires_auth(anon_client, world):
    assert anon_client.get(page_url(world, world["page"]["id"])).status_code in (401, 403)


def test_retrieve_guest_restricted_page(guest_client, world):
    # Seeded projects have guest_view_all_features=false: a guest may not
    # view another member's page.
    response = guest_client.get(page_url(world, world["page"]["id"]))
    assert response.status_code == 400
    assert response.json() == {"error": "You are not allowed to view this page"}


def test_retrieve_guest_allowed_when_flag_on(guest_client, world, db):
    db.execute("UPDATE projects SET guest_view_all_features=true WHERE id=%s", (world["project"]["id"],))
    assert guest_client.get(page_url(world, world["page"]["id"])).status_code == 200


def test_cross_workspace_detail_isolation(user_client, world, world2):
    # NOTE (ported behavior): world2's page id is unknown under world1's
    # slug, so the permission lookup raises DoesNotExist — mapped to 404 by
    # BaseViewSet.handle_exception. Either way the foreign page must never
    # render here.
    response = user_client.get(page_url(world, world2["page"]["id"]))
    assert response.status_code == 404
    assert response.json() == {"error": "The required object does not exist."}


def test_cross_workspace_list_isolation(user_client, world, world2):
    rows = user_client.get(page_url(world)).json()
    assert all(row["workspace"] == world["workspace"]["id"] for row in rows)
    assert world2["page"]["id"] not in {row["id"] for row in rows}


def test_partial_update_shape(user_client, world, db):
    response = user_client.patch(page_url(world, world["page"]["id"]), json={"name": "Renamed page"})
    assert response.status_code == 200
    body = response.json()
    # partial_update returns the update-bound PageDetailSerializer (no
    # issue_ids — only retrieve adds those; no annotation-backed keys either,
    # see PATCH_ROW_KEYS), so pin that exact key set.
    assert set(body) == PATCH_ROW_KEYS, f"patch keys drifted: {sorted(body)}"
    assert body["name"] == "Renamed page"
    assert db.fetchval("SELECT name FROM pages WHERE id=%s", (world["page"]["id"],)) == "Renamed page"


def test_partial_update_locked_page(user_client, world, seeder):
    locked = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Locked", locked=True)
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], locked["id"])
    response = user_client.patch(page_url(world, locked["id"]), json={"name": "Nope"})
    assert response.status_code == 400
    assert response.json() == {"error": "Page is locked"}


def test_partial_update_access_by_non_owner(member_client, world):
    # Only the page owner may change access, even for members.
    response = member_client.patch(page_url(world, world["page"]["id"]), json={"access": 1})
    assert response.status_code == 400
    assert response.json() == {
        "error": "Access cannot be updated since this page is owned by someone else"
    }


def test_partial_update_unlinked_page_reports_owner_error(user_client, world, seeder):
    # NOTE (ported quirk): a page that exists in the workspace but is not
    # linked to the URL project surfaces the owner-access error, not a 404,
    # because partial_update catches DoesNotExist into that body. (A fully
    # unknown id never reaches the action: the permission lookup raises first,
    # so it 404s like retrieve — see test_retrieve_unknown_page_id_is_404.)
    other_project = seeder.create_project(world["workspace"]["id"])
    seeder.create_project_member(
        world["workspace"]["id"], other_project["id"], world["owner"]["id"], role=20
    )
    other = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Elsewhere")
    seeder.link_page_project(world["workspace"]["id"], other_project["id"], other["id"])
    response = user_client.patch(page_url(world, other["id"]), json={"name": "Ghost"})
    assert response.status_code == 400
    assert response.json() == {
        "error": "Access cannot be updated since this page is owned by someone else"
    }


def test_destroy_requires_archived_first(user_client, world):
    response = user_client.delete(page_url(world, world["page"]["id"]))
    assert response.status_code == 400
    assert response.json() == {"error": "The page should be archived before deleting"}


def test_destroy_archived_page(user_client, world, db):
    assert user_client.post(f"{page_url(world, world['page']['id'])}archive/").status_code == 200
    assert user_client.delete(page_url(world, world["page"]["id"])).status_code == 204
    assert (
        db.fetchval("SELECT deleted_at FROM pages WHERE id=%s", (world["page"]["id"],)) is not None
    )
    # NOTE (ported behavior): destroy soft-deletes, and the default manager
    # hides soft-deleted rows — so the permission lookup raises DoesNotExist
    # and the detail view 404s (via BaseViewSet.handle_exception) after a
    # delete instead of returning the view's "Page not found" body.
    post_delete = user_client.get(page_url(world, world["page"]["id"]))
    assert post_delete.status_code == 404
    assert post_delete.json() == {"error": "The required object does not exist."}


def test_destroy_forbidden_for_member_non_owner(member_client, world, seeder):
    # Members may not DELETE at all: the permission layer denies the method
    # before the view's owner/admin check is ever reached.
    archived = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Archived", archived=True
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], archived["id"])
    assert member_client.delete(page_url(world, archived["id"])).status_code == 403


def test_summary_shape(user_client, world, seeder):
    private = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Private", access=1)
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], private["id"])
    archived = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Archived", archived=True
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], archived["id"])
    response = user_client.get(f"{pages_base(world)}/pages-summary/")
    assert response.status_code == 200
    assert response.json() == {"public_pages": 1, "private_pages": 1, "archived_pages": 1}


def test_summary_requires_auth(anon_client, world):
    assert anon_client.get(f"{pages_base(world)}/pages-summary/").status_code in (401, 403)
