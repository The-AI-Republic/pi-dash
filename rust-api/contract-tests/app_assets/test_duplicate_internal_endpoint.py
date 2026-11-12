# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Duplicate-assets through the internal S3 endpoint (PIDASHCONV-1684).

Black-box port of Django main ``test_asset_duplicate_app.py``
(NEWFRONT-1028, PR #2766): the duplicate is a server-side S3 copy, so
the endpoint signs against the configured internal endpoint
(``AWS_S3_ENDPOINT_URL``) even when the public host the client reached
differs. With ``USE_MINIO=1`` and a request ``Host`` that is not
routable from the API container, pre-fix code answers 500 with the new
row left unflipped; fixed code answers 200, flips the row, and the
destination object carries the source bytes.

Environment: the server under test must boot with ``USE_MINIO=1`` and
a live ``AWS_S3_ENDPOINT_URL`` (LocalStack in CI, like the auth_oauth
leg); the tests create the contract bucket themselves. The matrix job
boots app_assets without MinIO, and its Django predates NEWFRONT-1028
until PIDASHCONV-1055 merges main into rust-dev, so this file is held
out in ``TASK_ORACLE_IGNORES.txt`` until that sync (the PIDASHCONV-1063
holding pattern). At the sync: provision LocalStack + ``USE_MINIO=1``
+ ``AWS_S3_ENDPOINT_URL`` for the app_assets leg (the auth_oauth
precedent in ``rust-api-contract-rust.yml``) and delete the ignore
line — without the MinIO boot env the file fails on both backends.

Cases (each starts from a staged source object in the bucket):

- duplicate with an unroutable ``Host`` → 200, the row flips, the
  destination object exists with the source bytes (ports
  ``test_duplicate_copies_through_internal_endpoint``, including the
  bucket / CopySource / key-shape assertions);
- duplicate with an unroutable ``Host`` → the clone carries the
  original's attributes/size/workspace with a fresh
  ``<workspace_id>/<hex>-<name>`` key while the source object stays
  put (ports ``test_duplicate_builds_server_storage_client``);
- download with a foreign ``Host`` → the 302 still signs against the
  request host, not the internal endpoint (ports
  ``test_download_still_signs_against_request_host``; passes before
  and after the fix — a guard on the presign paths).
"""

from urllib.parse import quote, urlparse

from _harness import s3

from . import seed_assets as seed_a

SOURCE_BODY = b"shot-bytes-1684"

# Routable nowhere: nothing listens on the discard port, so a copy
# through the request host dies as a transport failure (→ 500 with the
# row unflipped), while a copy through the internal endpoint succeeds.
UNROUTABLE_HOST = "127.0.0.1:9"


def dup_path(org, asset_id):
    return (
        f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        f"/duplicate-assets/{asset_id}/"
    )


def mint_uploaded(org, name="shot.png"):
    r = org.request(
        "POST", f"/api/assets/v2/workspaces/{org.workspace['slug']}/", "admin",
        json={"name": name, "type": "image/png", "size": len(SOURCE_BODY),
              "entity_type": "WORKSPACE_LOGO",
              "entity_identifier": org.workspace["id"]},
    )
    assert r.status_code == 200, r.text
    asset_id = r.json()["asset_id"]
    seed_a.mark_uploaded(org.conn, asset_id)
    return asset_id


def stage_source(org, asset_id):
    bucket = s3.contract_bucket()
    s3.s3_setup_bucket(bucket)
    key = seed_a.fetch(org.conn, asset_id)["asset"]
    s3.s3_put_key(bucket, key, SOURCE_BODY)
    return bucket, key


def duplicate(org, asset_id, host):
    return org.request(
        "POST", dup_path(org, asset_id), "admin",
        json={"entity_type": "WORKSPACE_LOGO",
              "entity_id": org.workspace["id"],
              "project_id": org.project["id"]},
        headers={"Host": host},
    )


def test_duplicate_copies_through_internal_endpoint(org):
    asset_id = mint_uploaded(org)
    bucket, _ = stage_source(org, asset_id)
    r = duplicate(org, asset_id, UNROUTABLE_HOST)
    assert r.status_code == 200, r.text
    assert set(r.json().keys()) == {"asset_id"}
    clone = seed_a.fetch(org.conn, r.json()["asset_id"])
    assert clone["is_uploaded"] is True
    assert clone["asset"].startswith(f"{org.workspace['id']}/")
    assert clone["asset"].endswith("-shot.png")
    dest = clone["asset"]
    assert s3.s3_key_exists(bucket, dest), "copy never reached the internal endpoint"
    got = s3.s3_call("GET", f"{s3.s3_endpoint()}/{bucket}/{quote(dest, safe='/~')}")
    assert got.status_code == 200, got.text
    assert got.content == SOURCE_BODY


def test_duplicate_server_copy_row_effects(org):
    asset_id = mint_uploaded(org, name="page.png")
    bucket, source_key = stage_source(org, asset_id)
    original = seed_a.fetch(org.conn, asset_id)
    r = duplicate(org, asset_id, UNROUTABLE_HOST)
    assert r.status_code == 200, r.text
    clone = seed_a.fetch(org.conn, r.json()["asset_id"])
    assert clone["attributes"] == original["attributes"]
    assert clone["size"] == original["size"]
    assert clone["workspace_id"] == original["workspace_id"]
    assert clone["asset"] != original["asset"]
    assert clone["asset"].startswith(f"{org.workspace['id']}/")
    assert clone["asset"].endswith("-page.png")
    # A copy leaves the source object behind.
    assert s3.s3_key_exists(bucket, source_key)
    assert seed_a.fetch(org.conn, asset_id)["asset"] == source_key


def test_download_still_signs_against_request_host(org):
    asset_id = mint_uploaded(org)
    r = org.request(
        "GET",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}/download/{asset_id}/",
        "admin",
        headers={"Host": "assets.example.com"},
    )
    assert r.status_code == 302, r.text
    location = r.headers["location"]
    assert location.startswith("http://assets.example.com/"), location
    assert seed_a.fetch(org.conn, asset_id)["asset"] in location
    internal = urlparse(s3.s3_endpoint()).netloc
    assert urlparse(location).netloc != internal
