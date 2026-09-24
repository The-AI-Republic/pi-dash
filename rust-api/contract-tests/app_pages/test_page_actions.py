"""Contract: favorite, archive/unarchive, lock/unlock, access actions.

Covers ``app/urls/page.py`` for ``PageFavoriteViewSet`` and the
``PageViewSet`` detail actions ``archive``, ``lock`` and ``access``.

Favorites are gated by role (ADMIN/MEMBER only — the guest case is the
denied-permission tripwire here); archive/unarchive additionally require
ownership or an admin role.
"""

import pytest

from _harness.auth import login_session
from _harness.http import api_client

from .conftest import page_url

pytestmark = pytest.mark.contract


def _fav_url(world, page_id):
    return f"/api/workspaces/{world['workspace']['slug']}/projects/{world['project']['id']}/favorite-pages/{page_id}/"


def test_favorite_create(user_client, world, db):
    assert user_client.post(_fav_url(world, world["page"]["id"])).status_code == 204
    row = db.fetchone(
        "SELECT user_id, deleted_at FROM user_favorites WHERE entity_type='page' AND entity_identifier=%s",
        (world["page"]["id"],),
    )
    assert row["user_id"] == world["owner"]["id"]
    assert row["deleted_at"] is None


def test_favorite_create_denied_for_guest(guest_client, world):
    # allow_permission([ADMIN, MEMBER]): guests cannot favorite.
    assert guest_client.post(_fav_url(world, world["page"]["id"])).status_code in (401, 403)


def test_favorite_create_requires_auth(anon_client, world):
    assert anon_client.post(_fav_url(world, world["page"]["id"])).status_code in (401, 403)


def test_favorite_destroy(user_client, world, db):
    assert user_client.post(_fav_url(world, world["page"]["id"])).status_code == 204
    assert user_client.delete(_fav_url(world, world["page"]["id"])).status_code == 204
    row = db.fetchval(
        "SELECT deleted_at FROM user_favorites WHERE entity_type='page' AND entity_identifier=%s",
        (world["page"]["id"],),
    )
    assert row is not None


def test_archive_shape(user_client, world, db):
    response = user_client.post(f"{page_url(world, world['page']['id'])}archive/")
    assert response.status_code == 200
    assert set(response.json()) == {"archived_at"}
    assert db.fetchval("SELECT archived_at FROM pages WHERE id=%s", (world["page"]["id"],)) is not None


def test_archive_covers_descendants(user_client, world, seeder, db):
    child = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Child", parent_id=world["page"]["id"]
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], child["id"])
    assert user_client.post(f"{page_url(world, world['page']['id'])}archive/").status_code == 200
    assert db.fetchval("SELECT archived_at FROM pages WHERE id=%s", (child["id"],)) is not None


def test_archive_denied_for_member_non_owner(member_client, world):
    response = member_client.post(f"{page_url(world, world['page']['id'])}archive/")
    assert response.status_code == 400
    assert response.json() == {"error": "Only the owner or admin can archive the page"}


def test_archive_allowed_for_admin_non_owner(world, seeder, settings):
    admin2 = seeder.create_user()
    seeder.create_project_member(
        world["workspace"]["id"], world["project"]["id"], admin2["id"], role=20
    )
    with api_client(settings.base_url) as client:
        login_session(client, email=admin2["email"], password=admin2["password"])
        assert client.post(f"{page_url(world, world['page']['id'])}archive/").status_code == 200


def test_unarchive(user_client, world, db):
    assert user_client.post(f"{page_url(world, world['page']['id'])}archive/").status_code == 200
    assert user_client.delete(f"{page_url(world, world['page']['id'])}archive/").status_code == 204
    assert db.fetchval("SELECT archived_at FROM pages WHERE id=%s", (world["page"]["id"],)) is None


def test_unarchive_detaches_archived_parent(user_client, world, seeder, db):
    child = seeder.create_page(
        world["workspace"]["id"],
        world["owner"]["id"],
        name="Child",
        parent_id=world["page"]["id"],
        archived=True,
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], child["id"])
    db.execute("UPDATE pages SET archived_at=CURRENT_DATE WHERE id=%s", (world["page"]["id"],))
    assert user_client.delete(f"{page_url(world, child['id'])}archive/").status_code == 204
    row = db.fetchone("SELECT parent_id, archived_at FROM pages WHERE id=%s", (child["id"],))
    assert row["parent_id"] is None
    assert row["archived_at"] is None
    # The parent itself stays archived: unarchiving never cascades upward.
    assert (
        db.fetchval("SELECT archived_at FROM pages WHERE id=%s", (world["page"]["id"],)) is not None
    )


def test_unarchive_denied_for_member_non_owner(member_client, world, seeder):
    # Members may not DELETE at all: the permission layer denies the method
    # before the view's owner/admin check is ever reached.
    archived = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Archived", archived=True
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], archived["id"])
    assert member_client.delete(f"{page_url(world, archived['id'])}archive/").status_code == 403


def test_lock_and_unlock(user_client, world, db):
    assert user_client.post(f"{page_url(world, world['page']['id'])}lock/").status_code == 204
    assert db.fetchval("SELECT is_locked FROM pages WHERE id=%s", (world["page"]["id"],)) is True
    assert user_client.delete(f"{page_url(world, world['page']['id'])}lock/").status_code == 204
    assert db.fetchval("SELECT is_locked FROM pages WHERE id=%s", (world["page"]["id"],)) is False


def test_lock_requires_auth(anon_client, world):
    assert anon_client.post(f"{page_url(world, world['page']['id'])}lock/").status_code in (401, 403)


def test_access_shape(user_client, world, db):
    assert user_client.post(f"{page_url(world, world['page']['id'])}access/", json={"access": 1}).status_code == 204
    assert db.fetchval("SELECT access FROM pages WHERE id=%s", (world["page"]["id"],)) == 1


def test_access_denied_for_non_owner(member_client, world):
    response = member_client.post(f"{page_url(world, world['page']['id'])}access/", json={"access": 1})
    assert response.status_code == 400
    assert response.json() == {
        "error": "Access cannot be updated since this page is owned by someone else"
    }


def test_access_requires_auth(anon_client, world):
    assert anon_client.post(f"{page_url(world, world['page']['id'])}access/").status_code in (401, 403)


def test_cross_workspace_action_isolation(user_client, world, world2, db):
    # NOTE (ported behavior): the permission lookup scopes by the URL
    # workspace, so another workspace's page id raises DoesNotExist there —
    # an uncaught 500. The foreign page must be left untouched either way.
    assert user_client.post(f"{page_url(world, world2['page']['id'])}lock/").status_code >= 500
    assert db.fetchval("SELECT is_locked FROM pages WHERE id=%s", (world2["page"]["id"],)) is False
