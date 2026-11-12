"""Contract: label and page endpoints (``api/urls/label.py``, ``api/urls/page.py``).

Page writes (create / update) require the live document service: without it
they fail with 503 and nothing is written. That 503 shape is asserted here
as the contract; reads and archive actions run against SQL-seeded pages.
The create tests below run a stub live server (``LiveDocStub``) so the
insert path itself is exercised; the stub is up only inside those tests.
"""

import uuid

import httpx
import pytest

from _harness.sinks import LiveDocStub

from .conftest import ENVELOPE_KEYS

pytestmark = pytest.mark.contract

LABEL_KEYS = {
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "color",
    "sort_order",
    "external_source",
    "external_id",
    "created_by",
    "updated_by",
    "workspace",
    "project",
    "parent",
}

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


def label_url(world, label_id=None):
    url = (
        f"/api/v1/workspaces/{world['workspace']['slug']}"
        f"/projects/{world['project']['id']}/labels/"
    )
    return url if label_id is None else f"{url}{label_id}/"


def page_url(world, page_id=None, action=None):
    url = (
        f"/api/v1/workspaces/{world['workspace']['slug']}"
        f"/projects/{world['project']['id']}/pages/"
    )
    if page_id is not None:
        url += f"{page_id}/"
    if action is not None:
        url += f"{action}/"
    return url


def test_label_create_list_shape(api, world):
    created = api.post(
        label_url(world),
        json={"name": "Bug", "color": "#ff0000", "description": "defects"},
    )
    assert created.status_code == 201, created.text
    assert set(created.json()) == LABEL_KEYS
    listed = api.get(label_url(world)).json()
    assert set(listed) == ENVELOPE_KEYS
    assert listed["total_results"] == 1
    assert set(listed["results"][0]) == LABEL_KEYS
    assert listed["results"][0]["name"] == "Bug"


def test_label_detail_patch_delete(api, world):
    label_id = api.post(label_url(world), json={"name": "Old", "color": "#000"}).json()["id"]
    assert set(api.get(label_url(world, label_id)).json()) == LABEL_KEYS
    patched = api.patch(label_url(world, label_id), json={"name": "New"})
    assert patched.status_code == 200
    assert patched.json()["name"] == "New"
    assert api.delete(label_url(world, label_id)).status_code == 204
    assert api.get(label_url(world, label_id)).status_code == 404


def test_page_list_shape(api, world, seeder):
    page = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Wiki home")
    seeder.link_page_to_project(page["id"], world["project"]["id"], world["workspace"]["id"])
    response = api.get(page_url(world))
    assert response.status_code == 200
    body = response.json()
    assert set(body) == ENVELOPE_KEYS
    assert body["total_results"] == 1
    assert set(body["results"][0]) == PAGE_LIST_KEYS


def test_page_detail_shape(api, world, seeder):
    page = seeder.create_page(world["workspace"]["id"], world["owner"]["id"])
    seeder.link_page_to_project(page["id"], world["project"]["id"], world["workspace"]["id"])
    response = api.get(page_url(world, page["id"]))
    assert response.status_code == 200
    body = response.json()
    assert set(body) == PAGE_DETAIL_KEYS, f"keys changed: {sorted(set(body) ^ PAGE_DETAIL_KEYS)}"
    assert body["id"] == page["id"]
    assert body["owned_by"] == world["owner"]["id"]


def test_page_create_without_doc_service_is_503(api, world):
    """No live document service here: create fails closed with 503."""
    response = api.post(
        page_url(world),
        json={"name": "Nope", "description_markdown": "# hi"},
    )
    assert response.status_code == 503
    assert set(response.json()) == {"error"}
    assert api.get(page_url(world)).json()["total_results"] == 0


def test_page_update_without_doc_service_is_503(api, world, seeder):
    page = seeder.create_page(world["workspace"]["id"], world["owner"]["id"])
    seeder.link_page_to_project(page["id"], world["project"]["id"], world["workspace"]["id"])
    response = api.patch(
        page_url(world, page["id"]), json={"description_markdown": "# edit"}
    )
    assert response.status_code == 503
    assert set(response.json()) == {"error"}


def test_page_archive_unarchive(api, world, seeder):
    page = seeder.create_page(world["workspace"]["id"], world["owner"]["id"])
    seeder.link_page_to_project(page["id"], world["project"]["id"], world["workspace"]["id"])
    archived = api.post(page_url(world, page["id"], "archive"))
    assert archived.status_code == 200, archived.text
    assert set(archived.json()) == PAGE_DETAIL_KEYS
    assert archived.json()["archived_at"] is not None
    restored = api.delete(page_url(world, page["id"], "archive"))
    assert restored.status_code == 200
    assert restored.json()["archived_at"] is None


def test_page_unarchive_finds_soft_deleted_archived_parent(api, world, seeder, db):
    # `page.parent` is an unscoped FK fetch (`_base_manager`): a soft-deleted
    # parent is FOUND and only its `archived_at` decides the detach
    # (PIDASHCONV-796). Still archived here, so the child detaches.
    parent = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Parent", archived=True
    )
    seeder.link_page_to_project(parent["id"], world["project"]["id"], world["workspace"]["id"])
    child = seeder.create_page(
        world["workspace"]["id"],
        world["owner"]["id"],
        name="Child",
        parent_id=parent["id"],
        archived=True,
    )
    seeder.link_page_to_project(child["id"], world["project"]["id"], world["workspace"]["id"])
    db.execute("UPDATE pages SET deleted_at=now() WHERE id=%s", (parent["id"],))
    restored = api.delete(page_url(world, child["id"], "archive"))
    assert restored.status_code == 200, restored.text
    assert restored.json()["archived_at"] is None
    assert restored.json()["parent"] is None


def test_page_unarchive_keeps_soft_deleted_live_parent(api, world, seeder, db):
    # Soft-deleted but NOT archived parent: found, no detach, still 200.
    parent = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Parent")
    seeder.link_page_to_project(parent["id"], world["project"]["id"], world["workspace"]["id"])
    child = seeder.create_page(
        world["workspace"]["id"],
        world["owner"]["id"],
        name="Child",
        parent_id=parent["id"],
        archived=True,
    )
    seeder.link_page_to_project(child["id"], world["project"]["id"], world["workspace"]["id"])
    db.execute("UPDATE pages SET deleted_at=now() WHERE id=%s", (parent["id"],))
    restored = api.delete(page_url(world, child["id"], "archive"))
    assert restored.status_code == 200, restored.text
    assert restored.json()["archived_at"] is None
    assert restored.json()["parent"] == parent["id"]


def test_page_unarchive_detach_writes_parent_only(api, world, seeder, settings, db):
    # Django's detach is ``page.parent = None; page.save(update_fields=["parent"])``
    # (api/views/page.py:566-571). ``update_fields`` restricts the UPDATE to that
    # one column, so neither ``updated_at`` (auto_now) nor ``updated_by``
    # (``BaseModel.save``) is persisted: only ``parent_id`` changes. The v1 port
    # used to stamp ``updated_at = now()`` and ``updated_by_id = requester`` on
    # the same UPDATE (PIDASHCONV-1386; the app_pages copy was fixed in 1169).
    parent = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Parent", archived=True
    )
    seeder.link_page_to_project(parent["id"], world["project"]["id"], world["workspace"]["id"])
    child = seeder.create_page(
        world["workspace"]["id"],
        world["owner"]["id"],
        name="Child",
        parent_id=parent["id"],
        archived=True,
    )
    seeder.link_page_to_project(child["id"], world["project"]["id"], world["workspace"]["id"])
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
    # Archived children are detail-visible: capture the rendered updated_at so
    # the response comparison below needs no format guess.
    detail_before = api.get(page_url(world, child["id"]))
    assert detail_before.status_code == 200, detail_before.text

    # The requester is a *different* eligible user (an ADMIN non-owner), so a
    # stray stamp would set updated_by_id to this admin, not to ``previous``.
    admin = seeder.create_user()
    seeder.create_workspace_member(world["workspace"]["id"], admin["id"], role=20)
    seeder.create_project_member(
        world["workspace"]["id"], world["project"]["id"], admin["id"], role=20
    )
    token = seeder.create_api_token(admin["id"], world["workspace"]["id"])
    with httpx.Client(
        base_url=settings.base_url, timeout=30, headers={"X-Api-Key": token["token"]}
    ) as admin_api:
        restored = admin_api.delete(page_url(world, child["id"], "archive"))
    assert restored.status_code == 200, restored.text
    assert restored.json()["archived_at"] is None
    assert restored.json()["parent"] is None
    assert restored.json()["updated_at"] == detail_before.json()["updated_at"]

    after = db.fetchone(
        "SELECT parent_id, updated_at, updated_by_id FROM pages WHERE id=%s", (child["id"],)
    )
    assert after["parent_id"] is None  # detached: only parent_id changed
    assert after["updated_at"] == before["updated_at"]
    assert str(after["updated_by_id"]) == str(before["updated_by_id"]) == previous["id"]


def _soft_delete_project(db, project_id, *, archived=False):
    """Soft-delete a seeded project, keeping its membership rows live.

    Mirrors the project-delete sweep window: ``delete(soft=True)`` stamps
    ``deleted_at`` first and the membership sweep lands later, so the
    permission gate still passes while every live-scoped project read
    misses (PIDASHCONV-795).
    """
    db.execute(
        "UPDATE projects SET deleted_at=now(),"
        " archived_at=CASE WHEN %s THEN now() ELSE NULL END WHERE id=%s",
        (archived, project_id),
    )


def test_page_create_deleted_project_is_503(api, world, db):
    """Without a live document service the 503 wins over the
    post-conversion live-project 404: the 795 check must sit after
    conversion, so a soft-deleted project still 503s here."""
    _soft_delete_project(db, world["project"]["id"])
    response = api.post(page_url(world), json={"name": "Gone"})
    assert response.status_code == 503, response.text
    assert set(response.json()) == {"error"}
    assert (
        db.fetchval(
            "SELECT count(*) FROM pages WHERE workspace_id=%s",
            (world["workspace"]["id"],),
        )
        == 0
    )


def test_page_create_deleted_archived_project_is_503_not_409(api, world, db):
    """The archived-project check is live-scoped (``Project.objects``): a
    soft-deleted+archived project never 409s (PIDASHCONV-795)."""
    _soft_delete_project(db, world["project"]["id"], archived=True)
    response = api.post(page_url(world), json={"name": "Gone"})
    assert response.status_code == 503, response.text
    assert set(response.json()) == {"error"}


def test_page_create_deleted_archived_project_bad_parent_is_400(api, world, db):
    """No 409 shadows the parent check on a soft-deleted+archived project."""
    _soft_delete_project(db, world["project"]["id"], archived=True)
    response = api.post(
        page_url(world), json={"name": "Gone", "parent": str(uuid.uuid4())}
    )
    assert response.status_code == 400, response.text
    assert response.json() == {"error": "Parent page not found in this project"}


def test_page_create_deleted_project_invalid_body_is_400(api, world, db):
    """Validation still runs first on a soft-deleted project."""
    _soft_delete_project(db, world["project"]["id"])
    response = api.post(page_url(world), json={})
    assert response.status_code == 400, response.text


def _conversion_hits(stub):
    """POSTs the backend made to the stub live server during the create."""
    return [hit for hit in stub.snapshot() if hit["method"] == "POST"]


def test_page_create_without_parent(api, world, db):
    """Create with no parent stores a null parent (PIDASHCONV-1503)."""
    with LiveDocStub() as stub:
        response = api.post(page_url(world), json={"name": "Top page"})
    assert response.status_code == 201, response.text
    body = response.json()
    assert set(body) == PAGE_DETAIL_KEYS, f"keys changed: {sorted(set(body) ^ PAGE_DETAIL_KEYS)}"
    assert body["name"] == "Top page"
    assert body["parent"] is None
    assert body["owned_by"] == world["owner"]["id"]
    assert api.get(page_url(world, body["id"])).json()["parent"] is None
    assert (
        db.fetchval("SELECT parent_id FROM pages WHERE id=%s", (body["id"],))
        is None
    )
    hits = _conversion_hits(stub)
    assert hits, "page create must convert through the live server"
    assert hits[0]["path"] == "/live/convert-document/"
    assert hits[0]["json"]["variant"] == "document"


def test_page_create_with_parent(api, world, seeder, db):
    """Create with a parent stores it (PIDASHCONV-1503: it was never written)."""
    parent = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Parent"
    )
    seeder.link_page_to_project(
        parent["id"], world["project"]["id"], world["workspace"]["id"]
    )
    with LiveDocStub() as stub:
        response = api.post(
            page_url(world), json={"name": "Child", "parent": parent["id"]}
        )
    assert response.status_code == 201, response.text
    body = response.json()
    assert set(body) == PAGE_DETAIL_KEYS, f"keys changed: {sorted(set(body) ^ PAGE_DETAIL_KEYS)}"
    assert body["parent"] == parent["id"]
    assert api.get(page_url(world, body["id"])).json()["parent"] == parent["id"]
    assert (
        str(db.fetchval("SELECT parent_id FROM pages WHERE id=%s", (body["id"],)))
        == parent["id"]
    )
    hits = _conversion_hits(stub)
    assert hits, "page create must convert through the live server"
    assert hits[0]["path"] == "/live/convert-document/"
    assert hits[0]["json"]["variant"] == "document"
