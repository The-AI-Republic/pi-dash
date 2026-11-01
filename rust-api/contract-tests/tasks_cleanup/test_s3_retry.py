# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-09 retry oracle: asset-task S3 calls against a scripted stub (PIDASHCONV-1697).

Stub environment only: the Django worker under test boots with
``AWS_S3_ENDPOINT_URL=http://127.0.0.1:<S3RETRY_S3_PORT>`` (default 4598)
and ``USE_MINIO=1``, with serial execution (``--pool=solo``) and no beat.
This module runs alone in its own CI job (see
``rust-api-contract-tests-tasks-cleanup-s3retry.yml``); the shared
worker-plane job ignores it — its worker points at live S3, and a live
endpoint would answer the copies instead of the script.

Django behavior (``settings/storage.py``, ``bgtasks/copy_s3_object.py``):
the boto3 client sets only the signature version, so botocore's legacy
retry mode applies — 5 attempts in total with ``rand * 2**(n-1)``
backoff — and a CopyObject 200 whose body is an ``<Error>`` is treated
as a 500 (``check_for_200_error``). Attempt counts below were measured
against boto3 1.34.96 / botocore 1.34.162; the Rust replay
(``test_rust_replay_s3_retry.py``) pins the identical counts. Counts are
path-filtered to the object key: a HEAD 400 makes botocore fire an extra
HeadBucket redirect probe, so HEAD scripts never answer 400 here.

Local run (own scratch database, Django worker only)::

    export DATABASE_URL=postgresql://... S3RETRY_S3_PORT=4598
    export AWS_S3_ENDPOINT_URL=http://127.0.0.1:4598 USE_MINIO=1
    export CELERY_BROKER_URL=redis://... # same broker the worker uses
    celery -A pi_dash worker -E --pool=solo -I <oracle includes> &
    pytest -p no:django tasks_cleanup/test_s3_retry.py
"""

import re
import uuid
from datetime import datetime, timedelta, timezone

from psycopg.rows import dict_row

from _harness import celery_wire
from _harness import s3stub
from _harness import seed as seed_helpers
from _harness.db import insert_row, wait_for_condition as wait_for

M = "pi_dash.bgtasks"

COPY_TASK = f"{M}.copy_s3_object.copy_s3_objects_of_description_and_assets"
METADATA_TASK = f"{M}.storage_metadata_task.get_asset_object_metadata"
SENTINEL_TASK = f"{M}.cleanup_task.delete_api_logs"

COPY_OK = ("respond", 200, {}, s3stub.COPY_OK_XML)


def _respond(status, code=None, body=None, headers=None):
    if body is None and code is not None:
        body = s3stub.s3_error_xml(code, f"stub {code}")
    return ("respond", status, headers or {}, body or b"")


def _object_attempts(stub, method):
    """Hits for object requests (HEAD/PUT on a key path, not HeadBucket)."""
    return sum(
        1
        for hit in stub.hits
        if hit.method == method and len(hit.path.split("/")) > 2
    )


def _page_html(conn, page_id) -> str:
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute('SELECT description_html FROM "pages" WHERE id = %s::uuid', (str(page_id),))
        return cur.fetchone()["description_html"]


def _asset_row(conn, asset_id):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            'SELECT id, asset, is_uploaded, storage_metadata FROM "file_assets"'
            " WHERE id = %s::uuid AND deleted_at IS NULL",
            (str(asset_id),),
        )
        return cur.fetchone()


def _workspace_duplicates(conn, workspace_id, source_id):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            'SELECT id, asset, is_uploaded FROM "file_assets"'
            " WHERE workspace_id = %s::uuid AND id != %s::uuid AND deleted_at IS NULL",
            (str(workspace_id), str(source_id)),
        )
        return cur.fetchall()


def _image_srcs(html: str) -> list:
    return re.findall(r'<image-component\s+src="([^"]+)"', html or "")


def _stored_paths(stub):
    return stub.stored_paths()


def _seed_copy_world(db_conn, tag: str) -> dict:
    """Page + source asset for the copy task (no live S3: the stub answers)."""
    chain = seed_helpers.issue_chain(db_conn, f"copy1697-{tag}")
    ws_id = str(chain["workspace"]["id"])
    proj_id = str(chain["project"]["id"])
    src_key = f"{ws_id}/copy1697-{tag}.png"
    asset = insert_row(
        db_conn,
        "file_assets",
        {
            "asset": src_key,
            "attributes": {"name": f"copy1697-{tag}.png", "type": "png", "size": 21},
            "size": 21.0,
            "workspace_id": ws_id,
            "project_id": proj_id,
            "created_by_id": str(chain["owner"]["id"]),
            "entity_type": "PAGE_DESCRIPTION",
            "is_uploaded": True,
            "is_deleted": False,
            "is_archived": False,
            "storage_metadata": {},
        },
    )
    seed_html = f'<p>t</p><image-component src="{asset["id"]}"></image-component>'
    page = insert_row(
        db_conn,
        "pages",
        {
            "workspace_id": ws_id,
            "owned_by_id": str(chain["owner"]["id"]),
            "name": f"copy1697-{tag}",
            "description_json": {},
            "description_html": seed_html,
            "access": 0,
            "color": "",
            "is_locked": False,
            "view_props": {},
            "logo_props": {},
            "is_global": False,
            "sort_order": 65535,
        },
    )
    return {
        "src_key": src_key,
        "asset_id": str(asset["id"]),
        "page_id": str(page["id"]),
        "project_id": proj_id,
        "owner_id": str(chain["owner"]["id"]),
        "slug": chain["workspace"]["slug"],
        "workspace_id": ws_id,
        "seed_html": seed_html,
    }


def _seed_head_world(db_conn, tag: str) -> dict:
    """One uploaded asset for the metadata task (no live S3)."""
    chain = seed_helpers.issue_chain(db_conn, f"head1697-{tag}")
    ws_id = str(chain["workspace"]["id"])
    key = f"{ws_id}/head1697-{tag}.png"
    asset = insert_row(
        db_conn,
        "file_assets",
        {
            "asset": key,
            "attributes": {"name": f"head1697-{tag}.png", "type": "png", "size": 21},
            "size": 21.0,
            "workspace_id": ws_id,
            "project_id": str(chain["project"]["id"]),
            "created_by_id": str(chain["owner"]["id"]),
            "entity_type": "PAGE_DESCRIPTION",
            "is_uploaded": True,
            "is_deleted": False,
            "is_archived": False,
            "storage_metadata": {},
        },
    )
    return {"asset_id": str(asset["id"]), "key": key}


def _publish_copy(world):
    celery_wire.publish(
        COPY_TASK,
        kwargs={
            "entity_name": "PAGE",
            "entity_identifier": world["page_id"],
            "project_id": world["project_id"],
            "slug": world["slug"],
            "user_id": world["owner_id"],
        },
    )


def _assert_recovered_copy(db_conn, world, stub):
    """The duplicate landed: rewritten src, uploaded row, stored object."""
    assert wait_for(
        lambda: _page_html(db_conn, world["page_id"]) != world["seed_html"],
        what="copy task rewrote the page description",
    )
    srcs = _image_srcs(_page_html(db_conn, world["page_id"]))
    assert srcs, "rewrite removed every image-component"
    assert world["asset_id"] not in srcs, "srcs still point at the source asset"
    stored = _stored_paths(stub)
    for src in srcs:
        row = _asset_row(db_conn, src)
        assert row is not None, f"src {src} resolves to no file_assets row"
        assert row["is_uploaded"] is True, f"duplicate {src} never marked uploaded"
        assert any(path.endswith(f"/{row['asset']}") for path in stored), (
            f"duplicate {src} points at S3 key {row['asset']} with no object behind it"
        )


def _assert_counted_copy_without_object(db_conn, world, stub):
    """Service-refused copy: the pair still counts, but nothing is stored."""
    assert wait_for(
        lambda: _page_html(db_conn, world["page_id"]) != world["seed_html"],
        what="copy task rewrote the page description",
    )
    srcs = _image_srcs(_page_html(db_conn, world["page_id"]))
    assert srcs, "rewrite removed every image-component"
    stored = _stored_paths(stub)
    for src in srcs:
        row = _asset_row(db_conn, src)
        assert row is not None, f"src {src} resolves to no file_assets row"
        assert row["is_uploaded"] is True, f"duplicate {src} never marked uploaded"
        assert not any(path.endswith(f"/{row['asset']}") for path in stored), (
            f"refused duplicate {src} unexpectedly stored {row['asset']}"
        )


def test_copy_first_try_success_single_attempt(db_conn, broker_url):
    """A first-try 200 costs one attempt; the duplicate's object lands."""
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    with s3stub.stub_s3([COPY_OK]) as stub:
        _publish_copy(world)
        _assert_recovered_copy(db_conn, world, stub)
        assert _object_attempts(stub, "PUT") == 1


def test_copy_recovers_after_503(db_conn, broker_url):
    """One 503 then success: Django retries and the duplicate's object lands."""
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    with s3stub.stub_s3([_respond(503, "SlowDown"), COPY_OK]) as stub:
        _publish_copy(world)
        _assert_recovered_copy(db_conn, world, stub)
        assert _object_attempts(stub, "PUT") == 2


def test_copy_recovers_after_200_error_body(db_conn, broker_url):
    """A 200 with an <Error> body is a failure: retried, then the copy lands."""
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    script = [_respond(200, "InternalError"), COPY_OK]
    with s3stub.stub_s3(script) as stub:
        _publish_copy(world)
        _assert_recovered_copy(db_conn, world, stub)
        assert _object_attempts(stub, "PUT") == 2


def test_copy_recovers_after_400_request_timeout(db_conn, broker_url):
    """S3's 400 RequestTimeout is retryable: the duplicate's object lands."""
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    script = [_respond(400, "RequestTimeout"), COPY_OK]
    with s3stub.stub_s3(script) as stub:
        _publish_copy(world)
        _assert_recovered_copy(db_conn, world, stub)
        assert _object_attempts(stub, "PUT") == 2


def test_copy_recovers_after_429(db_conn, broker_url):
    """A 429 is throttling: retried, then the duplicate's object lands."""
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    script = [_respond(429, "TooManyRequests"), COPY_OK]
    with s3stub.stub_s3(script) as stub:
        _publish_copy(world)
        _assert_recovered_copy(db_conn, world, stub)
        assert _object_attempts(stub, "PUT") == 2


def test_copy_404_single_attempt(db_conn, broker_url):
    """A 404 is final after one attempt; the pair still counts, minus object."""
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    with s3stub.stub_s3([_respond(404, "NoSuchKey")]) as stub:
        _publish_copy(world)
        _assert_counted_copy_without_object(db_conn, world, stub)
        assert _object_attempts(stub, "PUT") == 1


def test_copy_400_slowdown_single_attempt(db_conn, broker_url):
    """SlowDown on a 400 is not code-retried (legacy mode); one attempt."""
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    with s3stub.stub_s3([_respond(400, "SlowDown")]) as stub:
        _publish_copy(world)
        _assert_counted_copy_without_object(db_conn, world, stub)
        assert _object_attempts(stub, "PUT") == 1


def test_copy_503_always_five_attempts(db_conn, broker_url):
    """A stubborn 503 costs five attempts; the pair still counts, minus object."""
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    with s3stub.stub_s3([_respond(503, "SlowDown")]) as stub:
        _publish_copy(world)
        _assert_counted_copy_without_object(db_conn, world, stub)
        assert _object_attempts(stub, "PUT") == 5


def test_copy_conn_drop_five_attempts(db_conn, broker_url):
    """Dropped connections cost five attempts; the task takes the [] path.

    The duplicate row stays (partial writes are kept) but is never marked
    uploaded and the page is never rewritten. A sentinel job proves the
    worker drained the copy task: the [] path has no terminal DB write of
    its own, so the attempt count is only final once the sentinel lands.
    """
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    sentinel = insert_row(
        db_conn,
        "api_activity_logs",
        {
            "token_identifier": "retry1697-probe",
            "path": "/retry1697/drop-old",
            "method": "GET",
            "response_code": 200,
            "created_at": (datetime.now(timezone.utc) - timedelta(days=60)).isoformat(),
        },
    )

    def _sentinel_gone():
        with db_conn.cursor() as cur:
            cur.execute(
                'SELECT 1 FROM "api_activity_logs" WHERE id = %s::uuid',
                (str(sentinel["id"]),),
            )
            return cur.fetchone() is None

    with s3stub.stub_s3([("drop",)]) as stub:
        _publish_copy(world)
        celery_wire.publish(SENTINEL_TASK)
        wait_for(
            _sentinel_gone,
            what="sentinel api_activity_logs row deleted after dropped copy",
        )
        assert _object_attempts(stub, "PUT") == 5
    assert _page_html(db_conn, world["page_id"]) == world["seed_html"]
    duplicates = _workspace_duplicates(db_conn, world["workspace_id"], world["asset_id"])
    assert len(duplicates) == 1, f"expected the partial duplicate row, got {duplicates}"
    assert duplicates[0]["is_uploaded"] is False


def test_head_first_try_success_single_attempt(db_conn, broker_url):
    """A first-try 200 costs one attempt; the head dict is stored."""
    world = _seed_head_world(db_conn, uuid.uuid4().hex[:8])
    script = [("respond", 200, s3stub.head_ok_headers(21), b"")]
    with s3stub.stub_s3(script) as stub:
        celery_wire.publish(METADATA_TASK, kwargs={"asset_id": world["asset_id"]})
        assert wait_for(
            lambda: (_asset_row(db_conn, world["asset_id"]) or {}).get("storage_metadata") != {},
            what="metadata task saved storage_metadata",
        )
        assert _object_attempts(stub, "HEAD") == 1
    meta = _asset_row(db_conn, world["asset_id"])["storage_metadata"]
    assert meta is not None, "storage_metadata saved as SQL NULL"
    assert meta["ContentLength"] == 21


def test_head_recovers_after_503(db_conn, broker_url):
    """One 503 then success: Django retries and the head dict is stored."""
    world = _seed_head_world(db_conn, uuid.uuid4().hex[:8])
    script = [
        _respond(503, "SlowDown"),
        ("respond", 200, s3stub.head_ok_headers(21), b""),
    ]
    with s3stub.stub_s3(script) as stub:
        celery_wire.publish(METADATA_TASK, kwargs={"asset_id": world["asset_id"]})
        assert wait_for(
            lambda: (_asset_row(db_conn, world["asset_id"]) or {}).get("storage_metadata") != {},
            what="metadata task saved storage_metadata",
        )
        assert _object_attempts(stub, "HEAD") == 2
    meta = _asset_row(db_conn, world["asset_id"])["storage_metadata"]
    assert meta is not None, "storage_metadata saved as SQL NULL"
    assert set(meta) == {"ContentType", "ContentLength", "LastModified", "ETag", "Metadata"}
    assert meta["ContentLength"] == 21
    assert meta["ContentType"] == "application/octet-stream"
    assert meta["ETag"] == s3stub.HEAD_ETAG
    assert meta["LastModified"].endswith("+00:00"), meta["LastModified"]
    assert meta["Metadata"] == {}


def test_head_503_always_five_attempts(db_conn, broker_url):
    """A stubborn 503 costs five attempts; storage_metadata stays NULL."""
    world = _seed_head_world(db_conn, uuid.uuid4().hex[:8])
    with s3stub.stub_s3([_respond(503, "SlowDown")]) as stub:
        celery_wire.publish(METADATA_TASK, kwargs={"asset_id": world["asset_id"]})
        assert wait_for(
            lambda: (_asset_row(db_conn, world["asset_id"]) or {}).get("storage_metadata") != {},
            what="metadata task saved storage_metadata",
        )
        assert _object_attempts(stub, "HEAD") == 5
    assert _asset_row(db_conn, world["asset_id"])["storage_metadata"] is None


def test_head_404_single_attempt(db_conn, broker_url):
    """A 404 costs one attempt; storage_metadata stays NULL."""
    world = _seed_head_world(db_conn, uuid.uuid4().hex[:8])
    with s3stub.stub_s3([_respond(404, "NoSuchKey")]) as stub:
        celery_wire.publish(METADATA_TASK, kwargs={"asset_id": world["asset_id"]})
        assert wait_for(
            lambda: (_asset_row(db_conn, world["asset_id"]) or {}).get("storage_metadata") != {},
            what="metadata task saved storage_metadata",
        )
        assert _object_attempts(stub, "HEAD") == 1
    assert _asset_row(db_conn, world["asset_id"])["storage_metadata"] is None
