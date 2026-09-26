"""Contract: label and page endpoints (``api/urls/label.py``, ``api/urls/page.py``).

Page writes (create / update) require the live document service: without it
they fail with 503 and nothing is written. That 503 shape is asserted here
as the contract; reads and archive actions run against SQL-seeded pages.
"""

import pytest

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
