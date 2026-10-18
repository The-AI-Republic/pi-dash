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
    # psycopg returns the uuid column as UUID, the seeder tracks it as str.
    assert str(row["user_id"]) == world["owner"]["id"]
    assert row["deleted_at"] is None


def test_favorite_create_denied_for_guest(guest_client, world):
    # allow_permission([ADMIN, MEMBER]): guests cannot favorite.
    assert guest_client.post(_fav_url(world, world["page"]["id"])).status_code in (401, 403)


def test_favorite_create_requires_auth(anon_client, world):
    assert anon_client.post(_fav_url(world, world["page"]["id"])).status_code in (401, 403)


def test_favorite_destroy(user_client, world, db):
    assert user_client.post(_fav_url(world, world["page"]["id"])).status_code == 204
    assert user_client.delete(_fav_url(world, world["page"]["id"])).status_code == 204
    # NOTE (ported behavior): destroy hard-deletes (``.delete(soft=False)``),
    # so the row is gone — there is no deleted_at timestamp to pin.
    row = db.fetchone(
        "SELECT id FROM user_favorites WHERE entity_type='page' AND entity_identifier=%s",
        (world["page"]["id"],),
    )
    assert row is None


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


def test_unarchive_detach_writes_parent_only(world, seeder, settings, db):
    # Django's detach is ``page.parent = None; page.save(update_fields=["parent"])``
    # (base.py:360-362). ``update_fields`` restricts the UPDATE to that one
    # column, so neither ``updated_at`` (auto_now) nor ``updated_by``
    # (``BaseModel.save``) is persisted: only ``parent_id`` changes. Rust used
    # to stamp ``updated_at = now()`` and ``updated_by_id = requester`` on the
    # same UPDATE (PIDASHCONV-1169).
    parent = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Parent", archived=True
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], parent["id"])
    child = seeder.create_page(
        world["workspace"]["id"],
        world["owner"]["id"],
        name="Child",
        parent_id=parent["id"],
        archived=True,
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], child["id"])
    # Pin an audit identity distinct from the requester so a stray stamp is
    # unmistakable: ``previous`` owns the child's last touch.
    previous = seeder.create_user()
    db.execute(
        "UPDATE pages SET updated_at='2021-02-03 04:05:06+00', updated_by_id=%s WHERE id=%s",
        (previous["id"], child["id"]),
    )
    before = db.fetchone(
        "SELECT parent_id, updated_at, updated_by_id FROM pages WHERE id=%s", (child["id"],)
    )

    # The requester is a *different* eligible user (an ADMIN non-owner), so a
    # Rust stamp would set updated_by_id to this admin, not to ``previous``.
    admin = seeder.create_user()
    seeder.create_project_member(
        world["workspace"]["id"], world["project"]["id"], admin["id"], role=20
    )
    with api_client(settings.base_url) as client:
        login_session(client, email=admin["email"], password=admin["password"])
        assert client.delete(f"{page_url(world, child['id'])}archive/").status_code == 204

    after = db.fetchone(
        "SELECT parent_id, updated_at, updated_by_id FROM pages WHERE id=%s", (child["id"],)
    )
    assert after["parent_id"] is None  # detached: only parent_id changed
    assert after["updated_at"] == before["updated_at"]
    assert str(after["updated_by_id"]) == str(before["updated_by_id"]) == previous["id"]


def test_unarchive_finds_soft_deleted_archived_parent(user_client, world, seeder, db):
    # `page.parent` is an unscoped FK fetch (`_base_manager`): a soft-deleted
    # parent is FOUND and only its `archived_at` decides the detach
    # (PIDASHCONV-796). Still archived here, so the child detaches.
    parent = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Parent", archived=True
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], parent["id"])
    child = seeder.create_page(
        world["workspace"]["id"],
        world["owner"]["id"],
        name="Child",
        parent_id=parent["id"],
        archived=True,
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], child["id"])
    db.execute("UPDATE pages SET deleted_at=now() WHERE id=%s", (parent["id"],))
    assert user_client.delete(f"{page_url(world, child['id'])}archive/").status_code == 204
    row = db.fetchone("SELECT parent_id, archived_at FROM pages WHERE id=%s", (child["id"],))
    assert row["parent_id"] is None
    assert row["archived_at"] is None


def test_unarchive_keeps_soft_deleted_live_parent(user_client, world, seeder, db):
    # Soft-deleted but NOT archived parent: found, no detach, still 204.
    parent = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Parent"
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], parent["id"])
    child = seeder.create_page(
        world["workspace"]["id"],
        world["owner"]["id"],
        name="Child",
        parent_id=parent["id"],
        archived=True,
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], child["id"])
    db.execute("UPDATE pages SET deleted_at=now() WHERE id=%s", (parent["id"],))
    assert user_client.delete(f"{page_url(world, child['id'])}archive/").status_code == 204
    row = db.fetchone("SELECT parent_id, archived_at FROM pages WHERE id=%s", (child["id"],))
    assert str(row["parent_id"]) == parent["id"]
    assert row["archived_at"] is None


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


# -- access body handling: the raw `.get` root and the read order ------------
#
# ``PageViewSet.access`` (``base.py:271-289``) reads ``request.data`` and
# calls the *raw* ``.get("access", …)`` — no serializer — so a well-formed
# non-object body raises ``AttributeError`` inside the view and Django's
# ``handle_exception`` renders the generic 500 (the same root as PB-0485 on
# the page write paths). And because ``request.data`` is read at ``:272``
# *before* the project-scoped fetch at ``:273-278``, a malformed body is
# DRF's 400 ``ParseError`` even when that fetch would miss.

_ACCESS_NON_OBJECT_BODIES = ["[1]", '"x"', "5", "null"]


@pytest.mark.parametrize(
    "raw",
    _ACCESS_NON_OBJECT_BODIES,
    ids=["list", "string", "number", "null"],
)
def test_access_non_object_body_is_generic_500(user_client, world, db, raw):
    before = db.fetchval("SELECT access FROM pages WHERE id=%s", (world["page"]["id"],))
    response = user_client.post(
        f"{page_url(world, world['page']['id'])}access/",
        content=raw,
        headers={"Content-Type": "application/json"},
    )
    assert response.status_code == 500
    assert response.json() == {"error": "Something went wrong please try again later"}
    # the failed call never reaches the save: the row is untouched.
    assert db.fetchval("SELECT access FROM pages WHERE id=%s", (world["page"]["id"],)) == before


def test_access_parses_body_before_project_scoped_fetch(user_client, world, db):
    # A page the requester owns (so the permission/gate lookup passes) whose
    # project_pages link to the URL project is soft-deleted: the scoped fetch
    # at ``base.py:273-278`` would 404, but ``request.data`` is read first
    # (``:272``), so a malformed body still answers the 400 ParseError.
    db.execute(
        "UPDATE project_pages SET deleted_at=now() WHERE page_id=%s AND project_id=%s",
        (world["page"]["id"], world["project"]["id"]),
    )
    response = user_client.post(
        f"{page_url(world, world['page']['id'])}access/",
        content='{"access":',
        headers={"Content-Type": "application/json"},
    )
    assert response.status_code == 400
    body = response.json()
    assert set(body) == {"detail"}
    # DRF's ParseError detail; Django appends the decoder message
    # ("- Expecting value: line 1 column 11 (char 10)") while the Rust backend
    # pins the shared prefix (a pre-existing app_pages divergence, out of scope
    # here — see PIDASHCONV-1172).
    assert body["detail"].startswith("JSON parse error")


def test_cross_workspace_action_isolation(user_client, world, world2, db):
    # NOTE (ported behavior): the permission lookup scopes by the URL
    # workspace, so another workspace's page id raises DoesNotExist there —
    # mapped to 404 by BaseViewSet.handle_exception. The foreign page must
    # be left untouched either way.
    lock_response = user_client.post(f"{page_url(world, world2['page']['id'])}lock/")
    assert lock_response.status_code == 404
    assert lock_response.json() == {"error": "The required object does not exist."}
    assert db.fetchval("SELECT is_locked FROM pages WHERE id=%s", (world2["page"]["id"],)) is False
