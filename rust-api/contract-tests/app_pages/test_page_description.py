"""Contract: page description binary retrieve + patch.

Covers ``app/urls/page.py`` for ``PagesDescriptionViewSet``:
``pages/<page_id>/description/`` (GET streams the raw binary, PATCH runs the
validated binary/HTML/JSON update).

The GET response is a byte stream, not JSON — the suite pins the
content-type, the attachment disposition and the exact bytes.
"""

import base64

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


@pytest.mark.parametrize("blank", ["", "   "])
def test_description_update_blank_binary_500_row_unchanged(user_client, world, seeder, db, blank):
    """A blank/whitespace-only ``description_binary`` validates (``CharField``
    trims to ``""``) but the ``BinaryField`` save raises ``TypeError``: both
    backends answer the generic 500 and leave the stored row untouched, so a
    "clear" PATCH never wipes the document (PB-0474)."""
    binary = b"\x89contract-blank-binary-1174"
    page = seeder.create_page(
        world["workspace"]["id"],
        world["owner"]["id"],
        name="Blank binary",
        description_binary=binary,
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], page["id"])
    columns = (
        "description_binary, description_html, description_json, "
        "description_stripped, updated_at, updated_by_id"
    )

    def snapshot():
        row = db.fetchone(f"SELECT {columns} FROM pages WHERE id=%s", (page["id"],))
        return {**row, "description_binary": bytes(row["description_binary"])}

    before = snapshot()
    response = user_client.patch(_desc_url(world, page["id"]), json={"description_binary": blank})
    assert response.status_code == 500
    assert response.json() == {"error": "Something went wrong please try again later"}
    assert snapshot() == before
    assert (
        bytes(db.fetchval("SELECT description_binary FROM pages WHERE id=%s", (page["id"],)))
        == binary
    )


# PIDASHCONV-1177: Django's validate_binary_data decodes with
# ``errors="ignore"`` (undecodable bytes are *dropped*, not replaced), so a
# suspicious ASCII pattern split by an invalid byte still matches and the
# PATCH answers 400. The Rust port used ``String::from_utf8_lossy``, which
# substitutes U+FFFD and let every split pattern through (200, bytes
# stored). These vectors pin the Django behaviour on both backends.

SUSPICIOUS_SPLIT_PATTERNS = [b"<html", b"<!doctype", b"<script", b"javascript:", b"data:", b"<iframe"]
SUSPICIOUS_BINARY_ERROR = {
    "description_binary": ["Invalid binary data: Binary data contains suspicious content patterns"]
}


def _split_pattern(pattern: bytes) -> bytes:
    """``pattern`` with a 0xff (invalid UTF-8) byte inserted in the middle."""
    mid = len(pattern) // 2
    return b"AB" + pattern[:mid] + b"\xff" + pattern[mid:] + b">x"


@pytest.mark.parametrize("pattern", SUSPICIOUS_SPLIT_PATTERNS)
def test_description_update_binary_suspicious_pattern_split_by_invalid_byte(
    user_client, world, seeder, db, pattern
):
    page = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Split pattern")
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], page["id"])
    payload = _split_pattern(pattern)
    response = user_client.patch(
        _desc_url(world, page["id"]),
        json={"description_binary": base64.b64encode(payload).decode()},
    )
    assert response.status_code == 400
    assert response.json() == SUSPICIOUS_BINARY_ERROR
    assert db.fetchval("SELECT description_binary FROM pages WHERE id=%s", (page["id"],)) is None


def test_description_update_binary_suspicious_pattern_split_by_invalid_byte_issue_example(
    user_client, world, seeder, db
):
    # base64 of b"AB<scr\xffipt>alert(1)</scr\xffipt>" — the exact vector from
    # the issue.
    page = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Issue example")
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], page["id"])
    response = user_client.patch(
        _desc_url(world, page["id"]),
        json={"description_binary": "QUI8c2Ny/2lwdD5hbGVydCgxKTwvc2Ny/2lwdD4="},
    )
    assert response.status_code == 400
    assert response.json() == SUSPICIOUS_BINARY_ERROR
    assert db.fetchval("SELECT description_binary FROM pages WHERE id=%s", (page["id"],)) is None


# PIDASHCONV-1689: page body nodes (PAGE-040 embeds, PAGE-043 entity
# references) survive the description_html sanitizer with their allowlisted
# attributes. Vectors mirror test_content_validator.py; (input, stored)
# pairs were captured by executing validate_html_content on Django main
# (nh3 0.2.18). The Rust allowlist predated NEWFRONT-914 and stripped the
# elements from the stored HTML.

EMBED_KEEP_VECTORS = [
    # The issue vector.
    (
        '<p><issue-embed-component entity_identifier="i1" project_identifier="P"></issue-embed-component></p>',
        '<p><issue-embed-component entity_identifier="i1" project_identifier="P"></issue-embed-component></p>',
    ),
    # All embed identifiers survive.
    (
        '<p><issue-embed-component id="emb-1" entity_identifier="ISS-2" project_identifier="P" workspace_identifier="acme" entity_name="issue"></issue-embed-component></p>',
        '<p><issue-embed-component id="emb-1" entity_identifier="ISS-2" project_identifier="P" workspace_identifier="acme" entity_name="issue"></issue-embed-component></p>',
    ),
    # All entity-reference attributes survive, including data-label.
    (
        '<p>see <entity-reference-component entity_name="issue" entity_identifier="ISS-2" project_identifier="P" workspace_identifier="acme" project_id="p-1" data-label="Fix login"></entity-reference-component></p>',
        '<p>see <entity-reference-component entity_name="issue" entity_identifier="ISS-2" project_identifier="P" workspace_identifier="acme" project_id="p-1" data-label="Fix login"></entity-reference-component></p>',
    ),
    # A bare node round-trips rather than vanishing.
    (
        "<p><entity-reference-component></entity-reference-component></p>",
        "<p><entity-reference-component></entity-reference-component></p>",
    ),
]


@pytest.mark.parametrize("html,stored", EMBED_KEEP_VECTORS)
def test_description_update_html_keeps_embed_and_entity_reference(
    user_client, world, seeder, db, html, stored
):
    page = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Embed nodes")
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], page["id"])
    response = user_client.patch(_desc_url(world, page["id"]), json={"description_html": html})
    assert response.status_code == 200
    assert response.json() == {"message": "Updated successfully"}
    assert db.fetchval("SELECT description_html FROM pages WHERE id=%s", (page["id"],)) == stored


def test_description_update_html_strips_handlers_from_embed_nodes(user_client, world, seeder, db):
    page = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Embed hostile")
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], page["id"])
    response = user_client.patch(
        _desc_url(world, page["id"]),
        json={
            "description_html": (
                '<p><issue-embed-component id="e" entity_identifier="ISS-2" '
                'onclick="alert(1)">x</issue-embed-component>'
                '<entity-reference-component entity_name="issue" '
                'onerror="alert(1)">y</entity-reference-component></p>'
            )
        },
    )
    assert response.status_code == 200
    assert db.fetchval("SELECT description_html FROM pages WHERE id=%s", (page["id"],)) == (
        '<p><issue-embed-component id="e" entity_identifier="ISS-2">x</issue-embed-component>'
        '<entity-reference-component entity_name="issue">y</entity-reference-component></p>'
    )


@pytest.mark.parametrize(
    "html,stored",
    [
        ("<p>hi</p><script>alert(1)</script>", "<p>hi</p>"),
        ("<p><marquee>scrolling</marquee></p>", "<p>scrolling</p>"),
    ],
)
def test_description_update_html_still_strips_hostile_markup(
    user_client, world, seeder, db, html, stored
):
    page = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name="Hostile markup")
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], page["id"])
    response = user_client.patch(_desc_url(world, page["id"]), json={"description_html": html})
    assert response.status_code == 200
    assert db.fetchval("SELECT description_html FROM pages WHERE id=%s", (page["id"],)) == stored
