# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Instance-admin runner list (`GET /api/instances/runners/`, PDASHOSS01-303).

Pins `runner/admin_views.py` (`InstanceRunnerListEndpoint`): the
`InstanceAdminPermission` gate, the present-but-empty / unknown filter 400s,
the workspace slug filter, `-created_at` order, 50 rows per page with the
`page` fallback, and the exact row bytes.
"""

import json
import uuid
from datetime import datetime, timedelta, timezone

import pytest

URL = "/api/instances/runners/"
BASE = datetime(2026, 10, 1, 12, 0, 0, tzinfo=timezone.utc)


@pytest.fixture
def runners(world):
    """Seed runners under a workspace + project + pod; wipe them before the
    suite-wide reset deletes the workspace (runner/pod FKs are PROTECT/CASCADE
    on the Django side but plain FKs in SQL)."""
    db = world["db"]
    ws = db.make_workspace("Fleet", "fleet", world["admin"]["id"])
    proj = db.make_project(ws["id"], "FLT")
    pod_id = str(uuid.uuid4())
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                """INSERT INTO pod (id, name, description, is_default, workspace_id,
                       project_id, created_at, updated_at)
                   VALUES (%s,'pod-fleet','',false,%s,%s,now(),now())""",
                (pod_id, ws["id"], proj["id"]),
            )
        conn.commit()

    def make(name, *, owner=None, provisioning="manual", status="offline",
             runner_version="", dev_metadata=None, host_label="",
             created_at=BASE, last_heartbeat_at=None, revoked_at=None):
        rid = str(uuid.uuid4())
        with db.connect() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    """INSERT INTO runner
                           (id, owner_id, workspace_id, pod_id, name, host_label,
                            provisioning, visibility, status, capabilities,
                            os, arch, runner_version,
                            refresh_token_hash, refresh_token_fingerprint,
                            refresh_token_generation, previous_refresh_token_hash,
                            access_token_signing_key_version,
                            enrollment_token_hash, enrollment_token_fingerprint,
                            enrolled_at, revoked_at, revoked_reason,
                            dev_machine_id, dev_metadata, protocol_version,
                            last_heartbeat_at, created_at, updated_at)
                       VALUES (%s,%s,%s,%s,%s,%s,%s,0,%s,'[]',
                               '','',%s,
                               '','',0,'',1,
                               '','',
                               NULL,%s,'',
                               NULL,%s,1,
                               %s,%s,now())""",
                    (rid, owner or world["admin"]["id"], ws["id"], pod_id, name,
                     host_label, provisioning, status, runner_version, revoked_at,
                     json.dumps(dev_metadata if dev_metadata is not None else {}),
                     last_heartbeat_at, created_at),
                )
            conn.commit()
        return rid

    yield {"make": make, "workspace": ws}
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("DELETE FROM runner")
            cur.execute("DELETE FROM pod")
            cur.execute("DELETE FROM project_members")
            cur.execute("DELETE FROM projects")
        conn.commit()


def test_anonymous_is_401(anon_api):
    res = anon_api.get(URL)
    assert res.status_code == 401
    assert res.json() == {"detail": "Authentication credentials were not provided."}


def test_non_admin_is_403(member_api):
    res = member_api.get(URL)
    assert res.status_code == 403
    assert res.json() == {"detail": "You do not have permission to perform this action."}


def test_empty_list(admin_api):
    res = admin_api.get(URL)
    assert res.status_code == 200
    assert res.text == '{"page":1,"total":0,"results":[]}'


def test_row_bytes(admin_api, world, runners):
    rid = runners["make"](
        "bundled",
        provisioning="desktop_bundled",
        status="online",
        runner_version="1.2.3",
        host_label="mac mini",
        dev_metadata={"codex_version": "rust-v0.153.4", "other": 1},
        last_heartbeat_at=BASE + timedelta(seconds=5, microseconds=789),
        revoked_at=BASE + timedelta(days=1),
    )
    res = admin_api.get(URL)
    assert res.status_code == 200
    assert res.text == (
        '{"page":1,"total":1,"results":[{"id":"%s","name":"bundled",'
        '"workspace_slug":"fleet","owner_email":"admin@example.com",'
        '"host_label":"mac\\u2028mini","provisioning":"desktop_bundled",'
        '"status":"online","runner_version":"1.2.3","codex_version":"rust-v0.153.4",'
        '"last_heartbeat_at":"2026-10-01T12:00:05.000789+00:00",'
        '"created_at":"2026-10-01T12:00:00+00:00",'
        '"revoked_at":"2026-10-02T12:00:00+00:00"}]}' % rid
    )


def test_missing_codex_version_is_null(admin_api, runners):
    runners["make"]("plain")
    row = admin_api.get(URL).json()["results"][0]
    assert row["codex_version"] is None
    assert row["last_heartbeat_at"] is None
    assert row["revoked_at"] is None


@pytest.mark.parametrize(
    "query,error,detail",
    [
        ("provisioning=", "invalid_provisioning", ["desktop_bundled", "manual"]),
        ("provisioning", "invalid_provisioning", ["desktop_bundled", "manual"]),
        ("provisioning=bogus", "invalid_provisioning", ["desktop_bundled", "manual"]),
        ("provisioning=x&status=y", "invalid_provisioning", ["desktop_bundled", "manual"]),
        ("status=", "invalid_status", ["busy", "offline", "online", "revoked"]),
        ("status=ONLINE", "invalid_status", ["busy", "offline", "online", "revoked"]),
        ("provisioning=bogus&provisioning=manual&status=", "invalid_status",
         ["busy", "offline", "online", "revoked"]),
    ],
)
def test_bad_filters_are_400(admin_api, query, error, detail):
    res = admin_api.get(f"{URL}?{query}")
    assert res.status_code == 400
    assert res.json() == {"error": error, "detail": detail}


def test_filters(admin_api, world, runners):
    make = runners["make"]
    make("m-off")
    make("m-on", status="online")
    make("bundled", provisioning="desktop_bundled", status="online")

    def names(params):
        body = admin_api.get(URL, params=params).json()
        return body["total"], sorted(r["name"] for r in body["results"])

    assert names({"provisioning": "desktop_bundled"}) == (1, ["bundled"])
    assert names({"provisioning": "manual"}) == (2, ["m-off", "m-on"])
    assert names({"status": "online"}) == (2, ["bundled", "m-on"])
    assert names({"provisioning": "manual", "status": "online"}) == (1, ["m-on"])
    assert names({"workspace": "fleet"}) == (3, ["bundled", "m-off", "m-on"])
    assert names({"workspace": ""}) == (3, ["bundled", "m-off", "m-on"])
    assert names({"workspace": "nope"}) == (0, [])


def test_newest_first(admin_api, runners):
    runners["make"]("old", created_at=BASE - timedelta(days=2))
    runners["make"]("new", created_at=BASE)
    runners["make"]("mid", created_at=BASE - timedelta(days=1))
    names = [r["name"] for r in admin_api.get(URL).json()["results"]]
    assert names == ["new", "mid", "old"]


def test_pages_of_50(admin_api, runners):
    for i in range(51):
        runners["make"](f"r{i:02d}", created_at=BASE + timedelta(minutes=i))
    first = admin_api.get(URL).json()
    assert first["page"] == 1 and first["total"] == 51
    assert len(first["results"]) == 50
    assert first["results"][0]["name"] == "r50"
    second = admin_api.get(URL, params={"page": "2"}).json()
    assert second["page"] == 2 and second["total"] == 51
    assert [r["name"] for r in second["results"]] == ["r00"]
    third = admin_api.get(URL, params={"page": "3"}).json()
    assert third == {"page": 3, "total": 51, "results": []}


@pytest.mark.parametrize(
    "raw,page", [("abc", 1), ("", 1), ("0", 1), ("-3", 1), ("2.0", 1), (" 2 ", 2), ("1_0", 10)]
)
def test_page_fallback(admin_api, runners, raw, page):
    runners["make"]("only")
    body = admin_api.get(URL, params={"page": raw}).json()
    assert body["page"] == page
    assert body["total"] == 1
    assert len(body["results"]) == (1 if page == 1 else 0)
