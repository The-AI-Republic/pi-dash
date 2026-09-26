"""Contract: page versions list/detail + duplicate.

Covers ``app/urls/page.py`` for ``PageVersionEndpoint``
(``pages/<page_id>/versions/`` and ``versions/<pk>/``) and
``PageDuplicateEndpoint`` (``pages/<page_id>/duplicate/``).

Versions are seeded with a NULL binary: DRF has no ``BinaryField`` mapping
(it falls back to a raw field), so the suite pins the NULL-binary shape it
can verify statically and leaves non-NULL binary encoding unpinned.
"""

import pytest

from .conftest import page_url

pytestmark = pytest.mark.contract

VERSION_ROW_KEYS = {
    "id",
    "workspace",
    "page",
    "last_saved_at",
    "owned_by",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
}

VERSION_DETAIL_KEYS = VERSION_ROW_KEYS | {"description_binary", "description_html", "description_json"}


def _versions_url(world, page_id, pk=None):
    base = f"{page_url(world, page_id)}versions/"
    return f"{base}{pk}/" if pk else base


def test_versions_list_shape(user_client, world, seeder):
    v1 = seeder.create_page_version(
        world["workspace"]["id"], world["page"]["id"], world["owner"]["id"], description_html="<p>v1</p>"
    )
    v2 = seeder.create_page_version(
        world["workspace"]["id"], world["page"]["id"], world["owner"]["id"], description_html="<p>v2</p>"
    )
    response = user_client.get(_versions_url(world, world["page"]["id"]))
    assert response.status_code == 200
    rows = response.json()
    assert isinstance(rows, list) and len(rows) == 2
    for row in rows:
        assert set(row) == VERSION_ROW_KEYS, f"version row keys drifted: {sorted(row)}"
        assert row["page"] == world["page"]["id"]
        assert row["owned_by"] == world["owner"]["id"]
    assert {row["id"] for row in rows} == {v1["id"], v2["id"]}


def test_versions_list_empty(user_client, world):
    assert user_client.get(_versions_url(world, world["page"]["id"])).json() == []


def test_versions_list_requires_auth(anon_client, world):
    assert anon_client.get(_versions_url(world, world["page"]["id"])).status_code in (401, 403)


def test_versions_list_isolated_across_workspaces(user_client, world, world2, seeder):
    # Versions are filtered by the URL workspace: world2's version must not
    # appear under world1's page (whose own version list stays empty).
    seeder.create_page_version(
        world2["workspace"]["id"], world2["page"]["id"], world2["owner"]["id"]
    )
    assert user_client.get(_versions_url(world, world["page"]["id"])).json() == []


def test_version_detail_shape(user_client, world, seeder):
    version = seeder.create_page_version(
        world["workspace"]["id"], world["page"]["id"], world["owner"]["id"], description_html="<p>v1</p>"
    )
    response = user_client.get(_versions_url(world, world["page"]["id"], version["id"]))
    assert response.status_code == 200
    body = response.json()
    assert set(body) == VERSION_DETAIL_KEYS, f"version detail keys drifted: {sorted(body)}"
    assert body["id"] == version["id"]
    assert body["description_html"] == "<p>v1</p>"
    assert body["description_json"] == {}
    assert body["description_binary"] is None


def test_version_detail_requires_auth(anon_client, world, seeder):
    version = seeder.create_page_version(
        world["workspace"]["id"], world["page"]["id"], world["owner"]["id"]
    )
    assert anon_client.get(_versions_url(world, world["page"]["id"], version["id"])).status_code in (
        401,
        403,
    )


def test_duplicate_shape(user_client, world, db):
    response = user_client.post(f"{page_url(world, world['page']['id'])}duplicate/")
    assert response.status_code == 201
    body = response.json()
    assert body["name"] == "Contract page (Copy)"
    assert body["owned_by"] == world["owner"]["id"]
    assert body["id"] != world["page"]["id"]
    assert body["project_ids"] == [world["project"]["id"]]
    # The detail serializer never exposes the raw binary column.
    assert "description_binary" not in body
    row = db.fetchone(
        "SELECT name, owned_by_id, description_binary FROM pages WHERE id=%s", (body["id"],)
    )
    assert row["name"] == "Contract page (Copy)"
    assert row["description_binary"] is None
    assert db.fetchval(
        "SELECT COUNT(*) FROM project_pages WHERE page_id=%s AND deleted_at IS NULL", (body["id"],)
    ) == db.fetchval(
        "SELECT COUNT(*) FROM project_pages WHERE page_id=%s AND deleted_at IS NULL",
        (world["page"]["id"],),
    )


def test_duplicate_private_page_denied_for_non_owner(member_client, world, seeder):
    # A private page's non-owner fails the permission check itself, so the
    # denial carries DRF's default body — never the view's "Permission denied".
    private = seeder.create_page(
        world["workspace"]["id"], world["owner"]["id"], name="Private", access=1
    )
    seeder.link_page_project(world["workspace"]["id"], world["project"]["id"], private["id"])
    assert member_client.post(f"{page_url(world, private['id'])}duplicate/").status_code == 403


def test_duplicate_requires_auth(anon_client, world):
    assert anon_client.post(f"{page_url(world, world['page']['id'])}duplicate/").status_code in (
        401,
        403,
    )
