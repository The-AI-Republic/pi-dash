"""Contract: page detail carries the stored ``description_json`` snapshot.

Port of Django ``TestAppPageDetailSnapshot``
(``apps/api/pi_dash/tests/contract/api/test_pages.py``, NEWFRONT-932): the
session detail read the page canvas uses additionally carries the stored
snapshot, and the record PATCH cannot overwrite it (the field is read-only
on ``PageDetailSerializer`` — snapshots are written only through the
description endpoint).
"""

import pytest

from .conftest import page_url

pytestmark = pytest.mark.contract

SNAPSHOT = {
    "type": "doc",
    "content": [
        {"type": "paragraph", "content": [{"type": "text", "text": "before embed"}]},
        {"type": "issue-embed-component", "attrs": {"entity_identifier": "ISS-1"}},
    ],
}


def test_detail_carries_the_stored_snapshot(user_client, world, seeder):
    page = seeder.create_page(
        world["workspace"]["id"],
        world["owner"]["id"],
        name="Snapshot page",
        description_html="<p>before embed</p>",
        description_json=SNAPSHOT,
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], page["id"])

    response = user_client.get(page_url(world, page["id"]), params={"track_visit": "false"})

    assert response.status_code == 200
    body = response.json()
    assert body["description_html"] == "<p>before embed</p>"
    assert body["description_json"] == SNAPSHOT


def test_record_patch_cannot_overwrite_the_snapshot(user_client, world, seeder, db):
    original = {"type": "doc", "content": []}
    page = seeder.create_page(
        world["workspace"]["id"],
        world["owner"]["id"],
        name="Patch page",
        description_json=original,
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], page["id"])

    response = user_client.patch(
        page_url(world, page["id"]),
        json={"name": "Renamed", "description_json": {"type": "doc", "content": [{"type": "paragraph"}]}},
    )

    assert response.status_code == 200
    body = response.json()
    assert body["name"] == "Renamed"
    assert body["description_json"] == original
    assert (
        db.fetchval("SELECT description_json FROM pages WHERE id=%s", (page["id"],)) == original
    )
