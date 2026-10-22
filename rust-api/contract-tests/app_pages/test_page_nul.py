"""Contract: NUL (U+0000) input in page text fields.

DRF 3.15.2 ``CharField.__init__`` appends ``ProhibitNullCharactersValidator``
(``rest_framework/fields.py:746``) after every length validator, so any NUL in
a serializer ``CharField`` fails with 400
``{"<field>": ["Null characters are not allowed."]}`` and nothing is stored.
Postgres cannot store NUL in ``text``, so without the validator the write
either 500s (create / page PATCH) or runs the sanitizer / base64 path
(description PATCH) instead of failing.

Covers ``PageSerializer``/``PageDetailSerializer`` ``name`` and ``color`` on
create and PATCH, ``PageDetailSerializer.description_html`` on PATCH, and
``PageBinaryUpdateSerializer`` ``description_binary``/``description_html`` on
the description PATCH (``app/views/page/base.py:521-575``).
"""

import pytest

from .conftest import page_url

pytestmark = pytest.mark.contract

NUL = "\u0000"
NULL_MSG = "Null characters are not allowed."


def _desc_url(world, page_id):
    return f"{page_url(world, page_id)}description/"


def test_create_rejects_nul_name(user_client, world, db):
    before = db.fetchval(
        "SELECT count(*) FROM pages WHERE workspace_id=%s", (world["workspace"]["id"],)
    )
    response = user_client.post(page_url(world), json={"name": f"ab{NUL}cd"})
    assert response.status_code == 400
    assert response.json() == {"name": [NULL_MSG]}
    # Nothing stored: the workspace still has only the seeded page.
    assert (
        db.fetchval("SELECT count(*) FROM pages WHERE workspace_id=%s", (world["workspace"]["id"],))
        == before
    )


def test_create_rejects_nul_color(user_client, world, db):
    before = db.fetchval(
        "SELECT count(*) FROM pages WHERE workspace_id=%s", (world["workspace"]["id"],)
    )
    response = user_client.post(page_url(world), json={"name": "x", "color": f"ab{NUL}cd"})
    assert response.status_code == 400
    assert response.json() == {"color": [NULL_MSG]}
    assert (
        db.fetchval("SELECT count(*) FROM pages WHERE workspace_id=%s", (world["workspace"]["id"],))
        == before
    )


def test_patch_rejects_nul_name(user_client, world, db):
    response = user_client.patch(page_url(world, world["page"]["id"]), json={"name": f"ab{NUL}cd"})
    assert response.status_code == 400
    assert response.json() == {"name": [NULL_MSG]}
    assert (
        db.fetchval("SELECT name FROM pages WHERE id=%s", (world["page"]["id"],))
        == "Contract page"
    )


def test_patch_rejects_nul_description_html(user_client, world, db):
    response = user_client.patch(
        page_url(world, world["page"]["id"]), json={"description_html": f"<p>ab{NUL}cd</p>"}
    )
    assert response.status_code == 400
    assert response.json() == {"description_html": [NULL_MSG]}
    assert (
        db.fetchval("SELECT description_html FROM pages WHERE id=%s", (world["page"]["id"],))
        == "<p></p>"
    )


def test_description_patch_rejects_nul_description_html(user_client, world, db):
    response = user_client.patch(
        _desc_url(world, world["page"]["id"]), json={"description_html": f"<p>ab{NUL}cd</p>"}
    )
    assert response.status_code == 400
    assert response.json() == {"description_html": [NULL_MSG]}
    assert (
        db.fetchval("SELECT description_html FROM pages WHERE id=%s", (world["page"]["id"],))
        == "<p></p>"
    )


def test_description_patch_rejects_nul_description_binary(user_client, world, db):
    response = user_client.patch(
        _desc_url(world, world["page"]["id"]), json={"description_binary": f"ab{NUL}cd"}
    )
    assert response.status_code == 400
    assert response.json() == {"description_binary": [NULL_MSG]}
    assert (
        db.fetchval("SELECT description_binary FROM pages WHERE id=%s", (world["page"]["id"],))
        is None
    )
