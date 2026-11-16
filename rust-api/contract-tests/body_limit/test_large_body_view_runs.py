# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Bodies under ``FILE_SIZE_LIMIT`` reach the view on both backends.

Django enforces exactly one request-body cap (``RequestBodySizeLimitMiddleware``,
``DATA_UPLOAD_MAX_MEMORY_SIZE`` = ``FILE_SIZE_LIMIT``, default 5242880); DRF's
parsers add no further size cap. ``BodyLimitLayer`` is Rust's sole enforcer
(PIDASHCONV-692), but several handlers re-read the body themselves with a
hardcoded 1-2.5 MiB cap and 500 (or silently empty it) on bodies Django serves
(PIDASHCONV-1308).

Each test posts a ~3 MiB JSON body (inside the 2-5 MiB window: over every
handler-local cap, under the 5242880 limit) as an authenticated project member
and asserts the view runs — 201/200 with the payload echoed back byte-identical,
not a 500. The suite runs unchanged against Django and against Rust (``BASE_URL``
selects the backend); passing on both runs is the parity proof. Pre-fix Rust
500s both routes (``detail_body`` / ``read_body`` map the ``to_bytes`` over-cap
failure to ``Denial::ServerError``).
"""

import json

import pytest

pytestmark = pytest.mark.contract

# ~3 MiB: over every handler-local cap (1/2/2.5 MiB), under FILE_SIZE_LIMIT.
PAD_LEN = 3_000_000
PAD = "x" * PAD_LEN


def _project_base(tenant: dict) -> str:
    return (
        f"/api/workspaces/{tenant['workspace']['slug']}"
        f"/projects/{tenant['project']['id']}"
    )


def test_cycle_create_with_3mb_description(make_tenant):
    """``POST .../cycles/`` with a 3 MiB description saves and echoes it."""
    tenant = make_tenant()
    payload = {"name": "c", "description": PAD}
    raw = json.dumps(payload).encode()
    assert 2 * 1024 * 1024 < len(raw) < 5_242_880, len(raw)
    resp = tenant["client"].post(
        f"{_project_base(tenant)}/cycles/",
        content=raw,
        headers={"Content-Type": "application/json"},
    )
    assert resp.status_code == 201, (resp.status_code, resp.content[:200])
    body = resp.json()
    assert body["name"] == "c"
    assert body["description"] == PAD


def test_user_properties_patch_with_3mb_filters(make_tenant):
    """``PATCH .../user-properties/`` with a 3 MiB field saves and echoes it."""
    tenant = make_tenant()
    payload = {"display_properties": {"key": PAD}}
    raw = json.dumps(payload).encode()
    assert 2 * 1024 * 1024 < len(raw) < 5_242_880, len(raw)
    resp = tenant["client"].patch(
        f"{_project_base(tenant)}/user-properties/",
        content=raw,
        headers={"Content-Type": "application/json"},
    )
    assert resp.status_code == 200, (resp.status_code, resp.content[:200])
    assert resp.json()["display_properties"] == {"key": PAD}
