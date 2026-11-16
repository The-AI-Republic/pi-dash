# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Asset write bodies honour DRF ``Request._parse`` content-type routing
(PIDASHCONV-1159).

Every v1 asset write endpoint reads ``request.data`` (user/server POST
``asset.py:117``, PATCH ``:217``/``:373``; generic POST ``:503``, PATCH
``:611``). None declares ``parser_classes``, so Django serves DRF's
default parsers and raises ``UnsupportedMediaType`` (415) the moment a
non-empty body carries anything but a JSON/form/multipart content type.
A ``text/plain`` body is 415 with the verbatim header in the detail; a
missing content-type is 415 too (runserver echoes ``text/plain`` while
prod Django and Rust echo ``""``, so the missing-header case asserts
status plus the message prefix/suffix, following the PIDASHCONV-1102
license precedent). ``DELETE`` never touches ``request.data`` and is out
of scope.
"""

import base64
import json
import os

import pytest

TEXT_PLAIN_415 = {"detail": 'Unsupported media type "text/plain" in request.'}

METADATA_TASK = "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata"

USER = "/api/v1/assets/user-assets/"
SERVER = "/api/v1/assets/user-assets/server/"
USER_POST = b'{"name":"probe.jpg","type":"image/jpeg","size":1024,"entity_type":"USER_AVATAR"}'
GENERIC_POST = b'{"name":"probe.pdf","type":"application/pdf","size":1024}'


def _generic_base(org):
    return f"/api/v1/workspaces/{org.workspace['slug']}/assets/"


def _mint(org, path, body, role="admin"):
    """Create an existing asset the PATCH cases need (Django looks it up
    before it reads ``request.data``)."""
    r = org.request(
        "POST", path, role, content=body, headers={"Content-Type": "application/json"}
    )
    assert r.status_code == 200, r.text[:400]
    return r.json()["asset_id"]


def _write_cases(org):
    user_id = _mint(org, USER, USER_POST)
    server_id = _mint(org, SERVER, USER_POST)
    generic_id = _mint(org, _generic_base(org), GENERIC_POST)
    return {
        "user-post": ("POST", USER, USER_POST),
        "user-patch": ("PATCH", f"{USER}{user_id}/", b'{"attributes":{}}'),
        "server-post": ("POST", SERVER, USER_POST),
        "server-patch": (
            "PATCH",
            f"/api/v1/assets/user-assets/{server_id}/server/",
            b'{"attributes":{}}',
        ),
        "generic-post": ("POST", _generic_base(org), GENERIC_POST),
        "generic-patch": (
            "PATCH",
            f"{_generic_base(org)}{generic_id}/",
            b'{"is_uploaded":true}',
        ),
    }


@pytest.fixture(params=["user-post", "user-patch", "server-post", "server-patch", "generic-post", "generic-patch"])
def write_case(request, org):
    return _write_cases(org)[request.param]


def test_text_plain_with_body_is_415(write_case, org):
    method, path, body = write_case
    r = org.request(method, path, "admin", content=body, headers={"Content-Type": "text/plain"})
    assert r.status_code == 415, (method, path, r.status_code, r.text[:300])
    assert r.json() == TEXT_PLAIN_415, (method, path, r.text[:300])


def test_missing_content_type_with_body_is_415(write_case, org):
    method, path, body = write_case
    req = org.client().build_request(
        method, path, content=body, headers={"X-Api-Key": org.tokens["admin"]}
    )
    assert "content-type" not in req.headers, (method, path)
    r = org.client().send(req)
    assert r.status_code == 415, (method, path, r.status_code, r.text[:300])
    assert r.text.startswith('{"detail":"Unsupported media type \\"')
    assert r.text.endswith('\\" in request."}')


def test_empty_body_with_non_json_content_type_is_not_415(org):
    # DRF's stream is empty at content-length zero, so `_parse` returns
    # ``{}`` without running a parser: validation runs, not 415.
    r = org.request(
        "POST", USER, "admin", content=b"", headers={"Content-Type": "text/plain"}
    )
    assert r.status_code != 415, r.text[:300]


def test_case_variant_json_content_type_serves(org):
    # DRF lowercases the media type base in `parse_header_parameters`, so
    # ``Application/JSON`` selects the JSONParser and the body serves.
    r = org.request(
        "POST", USER, "admin", content=USER_POST, headers={"Content-Type": "Application/JSON"}
    )
    assert r.status_code == 200, r.text[:300]


def test_form_urlencoded_body_keeps_proxying(org):
    # A form body selects DRF's FormParser: it parses (never 415) on both
    # backends, on Rust by proxying to Django.
    form = b"name=probe.jpg&type=image/jpeg&size=1024&entity_type=USER_AVATAR"
    r = org.request(
        "POST",
        USER,
        "admin",
        content=form,
        headers={"Content-Type": "application/x-www-form-urlencoded"},
    )
    assert r.status_code != 415, r.text[:300]


def _rust_queue_metadata_hits(org, asset_id):
    """``rust_job_queue`` rows for this asset (the Rust enqueue channel)."""
    reg = org.conn.execute("SELECT to_regclass('public.rust_job_queue') AS t").fetchone()
    if not reg or not reg[0]:
        return 0
    return org.conn.execute(
        "SELECT COUNT(*) FROM rust_job_queue WHERE task = %s AND kwargs->>'asset_id' = %s",
        (METADATA_TASK, asset_id),
    ).fetchone()[0]


def _celery_metadata_hits(asset_id):
    """``celery`` redis-list messages for this asset (the Django channel).

    ``None`` when the broker is not redis (the Rust-matrix run points
    ``AMQP_URL`` at RabbitMQ, where this path publishes nothing).
    """
    url = os.environ.get("AMQP_URL") or os.environ.get("CELERY_BROKER_URL", "")
    if not url.startswith("redis"):
        return None
    import redis

    hits = 0
    for raw in redis.Redis.from_url(url).lrange("celery", 0, -1):
        try:
            envelope = json.loads(raw)
        except ValueError:
            continue
        if envelope.get("headers", {}).get("task") != METADATA_TASK:
            continue
        # Celery protocol v2 base64-wraps the body on the redis transport
        # (kombu still reports content-encoding utf-8); try that first,
        # then the raw body.
        body = envelope.get("body", "")
        candidates = [body]
        if isinstance(body, str):
            try:
                candidates.insert(0, base64.b64decode(body).decode("utf-8"))
            except ValueError:
                pass
        for candidate in candidates:
            try:
                parsed = json.loads(candidate) if isinstance(candidate, str) else candidate
                _args, kwargs, _embed = parsed
                break
            except (ValueError, TypeError):
                continue
        else:
            continue
        if isinstance(kwargs, dict) and kwargs.get("asset_id") == asset_id:
            hits += 1
    return hits


@pytest.mark.parametrize("kind", ["user", "server"])
def test_rejected_patch_still_enqueues_metadata(org, kind):
    # PIDASHCONV-1408: Django enqueues get_asset_object_metadata BEFORE
    # DRF parses request.data (asset.py:214-217, server twin :370-373),
    # so a 415 PATCH still starts the metadata back-fill. The job lands
    # on exactly one channel per backend: the redis ``celery`` list on
    # Django, ``rust_job_queue`` on Rust (no worker runs in either
    # contract run, so nothing drains it). Other PATCH side effects
    # (process_logs, model_activity) carry the asset id in the URL, so
    # the match is on task name + asset_id kwarg, never a bare id.
    if kind == "user":
        asset_id = _mint(org, USER, USER_POST)
        path = f"{USER}{asset_id}/"
    else:
        asset_id = _mint(org, SERVER, USER_POST)
        path = f"/api/v1/assets/user-assets/{asset_id}/server/"
    cur = org.conn.execute(
        "UPDATE file_assets SET storage_metadata = '{}' WHERE id = %s", (asset_id,)
    )
    assert cur.rowcount == 1, (kind, asset_id)
    r = org.request(
        "PATCH", path, "admin", content=b"x", headers={"Content-Type": "text/plain"}
    )
    assert r.status_code == 415, (kind, r.status_code, r.text[:300])
    assert r.json() == TEXT_PLAIN_415, (kind, r.text[:300])
    queue_hits = _rust_queue_metadata_hits(org, asset_id)
    celery_hits = _celery_metadata_hits(asset_id)
    total = queue_hits + (celery_hits or 0)
    assert total == 1, (kind, asset_id, queue_hits, celery_hits)
