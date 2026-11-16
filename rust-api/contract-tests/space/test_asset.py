"""Contract: public asset (S3) endpoints.

Covers ``space/urls/asset.py`` (4 paths): the entity-asset GET/POST/PATCH/
DELETE quartet, restore, and bulk update.

``GET`` is ``AllowAny``; every mutation requires a session. Uploads never
touch real S3 here: POST mints a presigned URL offline and PATCH marks the row
uploaded, so the suite pins the HTTP contract without network side effects.
"""

import pytest

pytestmark = pytest.mark.contract

BASE = "/api/public"


def _asset_url(world, asset_id=None):
    url = f"{BASE}/assets/v2/anchor/{world['anchor']}/"
    return url if asset_id is None else f"{url}{asset_id}/"


def test_get_redirects_to_signed_url(anon_client, world):
    response = anon_client.get(_asset_url(world, world["asset"]["id"]), follow_redirects=False)
    assert response.status_code == 302
    assert response.headers.get("location")


def test_get_unknown_anchor_is_404(anon_client, world):
    response = anon_client.get(f"{BASE}/assets/v2/anchor/nope/{world['asset']['id']}/")
    assert response.status_code == 404
    assert response.json() == {"error": "Requested resource could not be found."}


def test_get_unuploaded_asset_is_404(anon_client, world, seeder):
    asset = seeder.create_asset(
        world["workspace"]["id"], world["project"]["id"], world["owner"]["id"], uploaded=False
    )
    response = anon_client.get(_asset_url(world, asset["id"]))
    assert response.status_code == 404
    assert response.json() == {"error": "The requested asset could not be found."}


def test_get_other_tenant_asset_is_not_found(anon_client, world, seeder):
    owner2 = seeder.create_user()
    workspace2 = seeder.create_workspace(owner2["id"])
    project2 = seeder.create_project(workspace2["id"])
    seeder.create_board(workspace2["id"], project2["id"])
    asset2 = seeder.create_asset(workspace2["id"], project2["id"], owner2["id"])
    response = anon_client.get(_asset_url(world, asset2["id"]))
    # Scoped to the anchor's workspace: cross-tenant ids do not resolve.
    assert response.status_code == 404


def test_post_requires_auth(anon_client, world):
    response = anon_client.post(
        _asset_url(world),
        data={"name": "shot.png", "type": "image/png", "size": "10", "entity_type": "ISSUE_DESCRIPTION"},
    )
    assert response.status_code in (401, 403)


def test_post_mints_upload(user_client, world):
    response = user_client.post(
        _asset_url(world),
        data={
            "name": "shot.png",
            "type": "image/png",
            "size": "10",
            "entity_type": "ISSUE_DESCRIPTION",
        },
    )
    assert response.status_code == 200
    body = response.json()
    assert set(body) == {"upload_data", "asset_id", "asset_url"}


def test_post_rejects_bad_entity_type(user_client, world):
    response = user_client.post(
        _asset_url(world),
        data={"name": "shot.png", "type": "image/png", "size": "10", "entity_type": "NOPE"},
    )
    assert response.status_code == 400
    assert response.json()["error"] == "Invalid entity type."


def test_post_rejects_bad_file_type(user_client, world):
    response = user_client.post(
        _asset_url(world),
        data={"name": "evil.exe", "type": "application/x-sh", "size": "10", "entity_type": "ISSUE_DESCRIPTION"},
    )
    assert response.status_code == 400
    assert "Invalid file type" in response.json()["error"]


def test_patch_marks_uploaded(user_client, world, seeder, db):
    asset = seeder.create_asset(
        world["workspace"]["id"], world["project"]["id"], world["owner"]["id"], uploaded=False
    )
    # Seeded metadata skips the Celery metadata probe; PATCH only flips flags.
    db.execute(
        "UPDATE file_assets SET storage_metadata='{\"seeded\": true}' WHERE id=%s", (asset["id"],)
    )
    response = user_client.patch(
        _asset_url(world, asset["id"]), json={"attributes": {"name": "renamed.png"}}
    )
    assert response.status_code == 204
    assert db.fetchval("SELECT is_uploaded FROM file_assets WHERE id=%s", (asset["id"],)) is True


def test_delete_and_restore(user_client, world, seeder):
    asset = seeder.create_asset(world["workspace"]["id"], world["project"]["id"], world["owner"]["id"])
    deleted = user_client.delete(_asset_url(world, asset["id"]))
    assert deleted.status_code == 204

    restored = user_client.post(f"{BASE}/assets/v2/anchor/{world['anchor']}/restore/{asset['id']}/")
    assert restored.status_code == 204


def test_bulk_links_comment_assets(user_client, world, seeder):
    comment_asset = seeder.create_asset(
        world["workspace"]["id"],
        world["project"]["id"],
        world["owner"]["id"],
        entity_type="COMMENT_DESCRIPTION",
    )
    response = user_client.post(
        f"{BASE}/assets/v2/anchor/{world['anchor']}/{world['comment']['id']}/bulk/",
        json={"asset_ids": [comment_asset["id"]]},
    )
    assert response.status_code == 204


def test_bulk_without_ids_is_400(user_client, world):
    response = user_client.post(
        f"{BASE}/assets/v2/anchor/{world['anchor']}/{world['comment']['id']}/bulk/",
        json={"asset_ids": []},
    )
    assert response.status_code == 400
    assert response.json() == {"error": "No asset ids provided."}
