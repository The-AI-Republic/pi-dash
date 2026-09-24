"""Contract: page description binary retrieve + patch.

Covers ``app/urls/page.py`` for ``PagesDescriptionViewSet``:
``pages/<page_id>/description/`` (GET streams the raw binary, PATCH runs the
validated binary/HTML/JSON update).

The GET response is a byte stream, not JSON — the suite pins the
content-type, the attachment disposition and the exact bytes.
"""

import pytest

from .conftest import page_url

pytestmark = pytest.mark.contract


def _desc_url(world, page_id):
    return f"{page_url(world, page_id)}description/"


def test_description_retrieve_binary(user_client, world, seeder):
    binary = b"\x89contract-binary-payload"
    page = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Binary page", description_binary=binary
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], page["id"])
    response = user_client.get(_desc_url(world, page["id"]))
    assert response.status_code == 200
    assert response.headers["content-type"] == "application/octet-stream"
    assert response.headers["content-disposition"] == 'attachment; filename="page_description.bin"'
    assert response.content == binary


def test_description_retrieve_empty(user_client, world):
    response = user_client.get(_desc_url(world, world["page"]["id"]))
    assert response.status_code == 200
    assert response.content == b""


def test_description_retrieve_requires_auth(anon_client, world):
    assert anon_client.get(_desc_url(world, world["page"]["id"])).status_code in (401, 403)


def test_description_update_html(user_client, world, db):
    response = user_client.patch(_desc_url(world, world["page"]["id"]), json={"description_html": "<p>hi</p>"})
    assert response.status_code == 200
    assert response.json() == {"message": "Updated successfully"}
    assert (
        db.fetchval("SELECT description_html FROM pages WHERE id=%s", (world["page"]["id"],))
        == "<p>hi</p>"
    )


def test_description_update_json(user_client, world, db):
    response = user_client.patch(
        _desc_url(world, world["page"]["id"]), json={"description_json": {"ops": []}}
    )
    assert response.status_code == 200
    assert db.fetchval("SELECT description_json FROM pages WHERE id=%s", (world["page"]["id"],)) == {
        "ops": []
    }


def test_description_update_locked_page(user_client, world, seeder):
    locked = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Locked", locked=True)
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], locked["id"])
    response = user_client.patch(_desc_url(world, locked["id"]), json={"description_html": "<p>x</p>"})
    assert response.status_code == 400
    assert response.json() == {"error_code": 4701, "error_message": "PAGE_LOCKED"}


def test_description_update_archived_page(user_client, world, seeder):
    archived = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Archived", archived=True
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], archived["id"])
    response = user_client.patch(
        _desc_url(world, archived["id"]), json={"description_html": "<p>x</p>"}
    )
    assert response.status_code == 400
    assert response.json() == {"error_code": 4702, "error_message": "PAGE_ARCHIVED"}


def test_description_update_requires_auth(anon_client, world):
    assert anon_client.patch(_desc_url(world, world["page"]["id"])).status_code in (401, 403)


def test_description_cross_workspace_isolation(user_client, world, world2):
    # NOTE (ported behavior): same permission-lookup 404 as the other detail
    # routes (PagesDescriptionViewSet is a BaseViewSet) — another
    # workspace's page id never resolves here.
    response = user_client.get(_desc_url(world, world2["page"]["id"]))
    assert response.status_code == 404
    assert response.json() == {"error": "The required object does not exist."}
