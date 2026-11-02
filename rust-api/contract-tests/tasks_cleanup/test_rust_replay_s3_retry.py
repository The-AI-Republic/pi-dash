# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-09 retry oracle, Rust-side replay (PIDASHCONV-1697).

Executable form of ``test_s3_retry.py`` against the Rust worker: the
``rust_job_queue`` row carries the exact Celery v2 payload (see
``_harness/rust_queue.py``) and the assertions are the same DB
before/after checks plus the stub attempt counts.

Stub environment only: the Rust worker under test boots with
``AWS_S3_ENDPOINT_URL=http://127.0.0.1:<S3RETRY_S3_PORT>`` (default 4598),
``USE_MINIO=1``, and ``--concurrency 1``. This module runs alone in its
own CI job (see ``rust-api-contract-tests-tasks-cleanup-s3retry.yml``);
the shared replay job pins the live-S3 file only, and the Django
worker-plane job ignores ``test_rust_replay*.py`` by glob.

Expected (Django behavior, botocore legacy retry): a transient S3
failure is retried up to five attempts with ``rand * 2**(n-1)``
backoff, and a CopyObject 200 with an ``<Error>`` body counts as a
failure. Before the fix the Rust store sent one attempt and treated any
2xx copy as success, so the recovery tests fail pre-fix (no object, one
attempt) and the always-fail tests fail on the attempt count.

Local run (own scratch database, Django worker STOPPED)::

    export DATABASE_URL=postgresql://... S3RETRY_S3_PORT=4598
    export AWS_S3_ENDPOINT_URL=http://127.0.0.1:4598 USE_MINIO=1
    pidash-api worker --concurrency 1 &  # Rust worker; Django worker stopped
    pytest -p no:django tasks_cleanup/test_rust_replay_s3_retry.py
"""

import re
import uuid
from datetime import datetime, timedelta, timezone

from psycopg.rows import dict_row

from _harness import rust_queue
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


def _seed_copy_world(db_conn, tag: str) -> dict:
    """Page + source asset for the copy task (no live S3: the stub answers)."""
    chain = seed_helpers.issue_chain(db_conn, f"rcopy1697-{tag}")
    ws_id = str(chain["workspace"]["id"])
    proj_id = str(chain["project"]["id"])
    src_key = f"{ws_id}/rcopy1697-{tag}.png"
    asset = insert_row(
        db_conn,
        "file_assets",
        {
            "asset": src_key,
            "attributes": {"name": f"rcopy1697-{tag}.png", "type": "png", "size": 21},
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
            "name": f"rcopy1697-{tag}",
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
    chain = seed_helpers.issue_chain(db_conn, f"rhead1697-{tag}")
    ws_id = str(chain["workspace"]["id"])
    key = f"{ws_id}/rhead1697-{tag}.png"
    asset = insert_row(
        db_conn,
        "file_assets",
        {
            "asset": key,
            "attributes": {"name": f"rhead1697-{tag}.png", "type": "png", "size": 21},
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
    rust_queue.publish(
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
    stored = stub.stored_paths()
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
    stored = stub.stored_paths()
    for src in srcs:
        row = _asset_row(db_conn, src)
        assert row is not None, f"src {src} resolves to no file_assets row"
        assert row["is_uploaded"] is True, f"duplicate {src} never marked uploaded"
        assert not any(path.endswith(f"/{row['asset']}") for path in stored), (
            f"refused duplicate {src} unexpectedly stored {row['asset']}"
        )


def test_copy_first_try_success_single_attempt(db_conn):
    """A first-try 200 costs one attempt; the duplicate's object lands."""
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    with s3stub.stub_s3([COPY_OK]) as stub:
        _publish_copy(world)
        _assert_recovered_copy(db_conn, world, stub)
        assert _object_attempts(stub, "PUT") == 1


def test_copy_recovers_after_503(db_conn):
    """One 503 then success: the worker retries and the object lands."""
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    with s3stub.stub_s3([_respond(503, "SlowDown"), COPY_OK]) as stub:
        _publish_copy(world)
        _assert_recovered_copy(db_conn, world, stub)
        assert _object_attempts(stub, "PUT") == 2


def test_copy_recovers_after_200_error_body(db_conn):
    """A 200 with an <Error> body is a failure: retried, then the copy lands."""
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    script = [_respond(200, "InternalError"), COPY_OK]
    with s3stub.stub_s3(script) as stub:
        _publish_copy(world)
        _assert_recovered_copy(db_conn, world, stub)
        assert _object_attempts(stub, "PUT") == 2


def test_copy_recovers_after_400_request_timeout(db_conn):
    """S3's 400 RequestTimeout is retryable: the duplicate's object lands."""
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    script = [_respond(400, "RequestTimeout"), COPY_OK]
    with s3stub.stub_s3(script) as stub:
        _publish_copy(world)
        _assert_recovered_copy(db_conn, world, stub)
        assert _object_attempts(stub, "PUT") == 2


def test_copy_recovers_after_429(db_conn):
    """A 429 is throttling: retried, then the duplicate's object lands."""
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    script = [_respond(429, "TooManyRequests"), COPY_OK]
    with s3stub.stub_s3(script) as stub:
        _publish_copy(world)
        _assert_recovered_copy(db_conn, world, stub)
        assert _object_attempts(stub, "PUT") == 2


def test_copy_404_single_attempt(db_conn):
    """A 404 is final after one attempt; the pair still counts, minus object."""
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    with s3stub.stub_s3([_respond(404, "NoSuchKey")]) as stub:
        _publish_copy(world)
        _assert_counted_copy_without_object(db_conn, world, stub)
        assert _object_attempts(stub, "PUT") == 1


def test_copy_400_slowdown_single_attempt(db_conn):
    """SlowDown on a 400 is not code-retried (legacy mode); one attempt."""
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    with s3stub.stub_s3([_respond(400, "SlowDown")]) as stub:
        _publish_copy(world)
        _assert_counted_copy_without_object(db_conn, world, stub)
        assert _object_attempts(stub, "PUT") == 1


def test_copy_503_always_five_attempts(db_conn):
    """A stubborn 503 costs five attempts; the pair still counts, minus object."""
    world = _seed_copy_world(db_conn, uuid.uuid4().hex[:8])
    with s3stub.stub_s3([_respond(503, "SlowDown")]) as stub:
        _publish_copy(world)
        _assert_counted_copy_without_object(db_conn, world, stub)
        assert _object_attempts(stub, "PUT") == 5


def test_copy_conn_drop_five_attempts(db_conn):
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
        rust_queue.publish(SENTINEL_TASK)
        wait_for(
            _sentinel_gone,
            what="sentinel api_activity_logs row deleted after dropped copy",
        )
        assert _object_attempts(stub, "PUT") == 5
    assert _page_html(db_conn, world["page_id"]) == world["seed_html"]
    duplicates = _workspace_duplicates(db_conn, world["workspace_id"], world["asset_id"])
    assert len(duplicates) == 1, f"expected the partial duplicate row, got {duplicates}"
    assert duplicates[0]["is_uploaded"] is False


def test_head_first_try_success_single_attempt(db_conn):
    """A first-try 200 costs one attempt; the head dict is stored."""
    world = _seed_head_world(db_conn, uuid.uuid4().hex[:8])
    script = [("respond", 200, s3stub.head_ok_headers(21), b"")]
    with s3stub.stub_s3(script) as stub:
        rust_queue.publish(METADATA_TASK, kwargs={"asset_id": world["asset_id"]})
        assert wait_for(
            lambda: (_asset_row(db_conn, world["asset_id"]) or {}).get("storage_metadata") != {},
            what="metadata task saved storage_metadata",
        )
        assert _object_attempts(stub, "HEAD") == 1
    meta = _asset_row(db_conn, world["asset_id"])["storage_metadata"]
    assert meta is not None, "storage_metadata saved as SQL NULL"
    assert meta["ContentLength"] == 21


def test_head_recovers_after_503(db_conn):
    """One 503 then success: the head dict is stored after a retry."""
    world = _seed_head_world(db_conn, uuid.uuid4().hex[:8])
    script = [
        _respond(503, "SlowDown"),
        ("respond", 200, s3stub.head_ok_headers(21), b""),
    ]
    with s3stub.stub_s3(script) as stub:
        rust_queue.publish(METADATA_TASK, kwargs={"asset_id": world["asset_id"]})
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


def test_head_503_always_five_attempts(db_conn):
    """A stubborn 503 costs five attempts; storage_metadata stays NULL."""
    world = _seed_head_world(db_conn, uuid.uuid4().hex[:8])
    with s3stub.stub_s3([_respond(503, "SlowDown")]) as stub:
        rust_queue.publish(METADATA_TASK, kwargs={"asset_id": world["asset_id"]})
        assert wait_for(
            lambda: (_asset_row(db_conn, world["asset_id"]) or {}).get("storage_metadata") != {},
            what="metadata task saved storage_metadata",
        )
        assert _object_attempts(stub, "HEAD") == 5
    assert _asset_row(db_conn, world["asset_id"])["storage_metadata"] is None


def test_head_404_single_attempt(db_conn):
    """A 404 costs one attempt; storage_metadata stays NULL."""
    world = _seed_head_world(db_conn, uuid.uuid4().hex[:8])
    with s3stub.stub_s3([_respond(404, "NoSuchKey")]) as stub:
        rust_queue.publish(METADATA_TASK, kwargs={"asset_id": world["asset_id"]})
        assert wait_for(
            lambda: (_asset_row(db_conn, world["asset_id"]) or {}).get("storage_metadata") != {},
            what="metadata task saved storage_metadata",
        )
        assert _object_attempts(stub, "HEAD") == 1
    assert _asset_row(db_conn, world["asset_id"])["storage_metadata"] is None
