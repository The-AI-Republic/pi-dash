"""Contract: ``description_stripped`` recompute decodes HTML entities.

``Page.save()`` / ``PageVersion.save()``
(``db/models/page.py:70-77``, ``:175-182``) recompute
``description_stripped`` with ``pi_dash.utils.html_processor.strip_tags``
(``MLStripper``, an ``HTMLParser`` with ``convert_charrefs=True``), which
DECODES entities: ``a &amp; b &lt;x&gt;`` -> ``a & b <x>``.

Six write paths in the Rust app_pages handlers used to recompute the column
with the port of ``django.utils.html.strip_tags`` (entities kept verbatim)
instead. This suite drives entity-bearing HTML through create, page PATCH,
description PATCH and lock and pins the decoded column, so the wrong helper
fails here on both backends.

Every case is one of the write paths named by the issue; the page PATCH and
description PATCH use a freshly seeded page so the stored column is the one
this request computed.
"""

import pytest

from .conftest import page_url

pytestmark = pytest.mark.contract

# ``strip_tags("<p>a &amp; b &lt;x&gt;</p>")``: the entity references resolve
# to their characters (``convert_charrefs=True``), the tags drop out.
ENTITY_HTML = "<p>a &amp; b &lt;x&gt;</p>"
ENTITY_STRIPPED = "a & b <x>"


def _stripped(db, page_id):
    return db.fetchval("SELECT description_stripped FROM pages WHERE id=%s", (page_id,))


def test_create_recomputes_stripped_with_decoded_entities(user_client, world, db):
    response = user_client.post(page_url(world), json={"name": "Entities", "description_html": ENTITY_HTML})
    assert response.status_code == 201
    page_id = response.json()["id"]
    assert _stripped(db, page_id) == ENTITY_STRIPPED


def test_page_patch_recomputes_stripped_with_decoded_entities(user_client, world, seeder, db):
    page = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Entities patch")
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], page["id"])
    response = user_client.patch(page_url(world, page["id"]), json={"description_html": ENTITY_HTML})
    assert response.status_code == 200
    assert _stripped(db, page["id"]) == ENTITY_STRIPPED


def test_description_patch_recomputes_stripped_with_decoded_entities(user_client, world, seeder, db):
    page = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Entities desc")
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], page["id"])
    response = user_client.patch(
        f"{page_url(world, page['id'])}description/", json={"description_html": ENTITY_HTML}
    )
    assert response.status_code == 200
    assert _stripped(db, page["id"]) == ENTITY_STRIPPED


def test_lock_recomputes_stripped_with_decoded_entities(user_client, world, seeder, db):
    # lock/unlock is a plain ``save()`` that recomputes the column from the
    # stored HTML; seed the HTML straight in and assert the POST decodes it.
    page = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Entities lock", description_html=ENTITY_HTML
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], page["id"])
    assert user_client.post(f"{page_url(world, page['id'])}lock/").status_code == 204
    assert _stripped(db, page["id"]) == ENTITY_STRIPPED
