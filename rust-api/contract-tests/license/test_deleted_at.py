# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Writable deleted_at on PATCH /api/instances/ and POST /api/instances/workspaces/.

PIDASHCONV-1099: neither InstanceSerializer nor WorkspaceSerializer marks
deleted_at read-only, so DRF maps it to a writable DateTimeField(allow_null)
and admins can soft-delete (or clear) through these endpoints. Every test
pins response body, row, and visibility on both backends.
"""

from datetime import datetime, timezone

UTC = timezone.utc

DELETED = "2026-01-01T00:00:00Z"
DELETED_DT = datetime(2026, 1, 1, tzinfo=UTC)
INVALID_MESSAGE = (
    "Datetime has wrong format. Use one of these formats instead: "
    "YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]."
)


def _instance_deleted_at(db):
    with db.connect() as conn:
        return conn.execute("select deleted_at from instances;").fetchone()[0]


def test_patch_instance_sets_deleted_at(admin_api, world):
    res = admin_api.patch("/api/instances/", json={"deleted_at": DELETED})
    assert res.status_code == 200
    assert res.json()["deleted_at"] == DELETED
    assert _instance_deleted_at(world["db"]) == DELETED_DT
    # Soft-deleted: the next GET takes the no-instance branch. The authed
    # client keeps this off the suite's shared anon throttle budget
    # (`anon: 30/minute`); the GET body is auth-independent (`AllowAny`).
    assert admin_api.get("/api/instances/").json() == {
        "is_activated": False,
        "is_setup_done": False,
    }


def test_patch_instance_deleted_at_with_offset(admin_api, world):
    res = admin_api.patch(
        "/api/instances/", json={"deleted_at": "2026-06-01T12:00:00+05:30"}
    )
    assert res.status_code == 200
    assert res.json()["deleted_at"] == "2026-06-01T06:30:00Z"
    assert _instance_deleted_at(world["db"]) == datetime(2026, 6, 1, 6, 30, tzinfo=UTC)


def test_patch_instance_deleted_at_naive_is_request_timezone(admin_api, world):
    # No offset: DRF makes the wall time aware in the request timezone
    # (TimezoneMixin activates the admin's zone; the harness seeds UTC).
    res = admin_api.patch("/api/instances/", json={"deleted_at": "2026-06-01T12:00:00"})
    assert res.status_code == 200
    assert res.json()["deleted_at"] == "2026-06-01T12:00:00Z"
    assert _instance_deleted_at(world["db"]) == datetime(2026, 6, 1, 12, 0, tzinfo=UTC)


def test_patch_instance_deleted_at_null_is_noop(admin_api, world):
    # A true clear (non-null -> null) is unobservable through PATCH: the
    # permission gate needs a live first() row, and a live row already has
    # deleted_at NULL. DRF still accepts the null; both backends no-op 200.
    res = admin_api.patch("/api/instances/", json={"deleted_at": None})
    assert res.status_code == 200
    assert res.json()["deleted_at"] is None
    assert _instance_deleted_at(world["db"]) is None
    body = admin_api.get("/api/instances/").json()
    assert set(body.keys()) == {"config", "instance"}


def test_patch_instance_deleted_at_invalid(admin_api, world):
    for bad in ("not-a-date", 123):
        res = admin_api.patch("/api/instances/", json={"deleted_at": bad})
        assert res.status_code == 400
        assert res.json() == {"deleted_at": [INVALID_MESSAGE]}
    assert _instance_deleted_at(world["db"]) is None


def test_patch_instance_deleted_at_error_order(admin_api):
    # DRF validates writable fields in model order; the audit column
    # deleted_at comes before the instance fields.
    res = admin_api.patch(
        "/api/instances/", json={"deleted_at": "bogus", "instance_name": "n" * 300}
    )
    assert res.status_code == 400
    assert list(res.json().keys()) == ["deleted_at", "instance_name"]


def test_create_workspace_with_deleted_at(admin_api, world):
    stamped = "2026-01-01T00:00:00.123456Z"
    res = admin_api.post(
        "/api/instances/workspaces/",
        json={"name": "Ghost", "slug": "ghost", "deleted_at": stamped},
    )
    assert res.status_code == 201
    assert res.json()["deleted_at"] == stamped
    with world["db"].connect() as conn:
        row = conn.execute(
            "select id, deleted_at from workspaces where slug = 'ghost';"
        ).fetchone()
        members = conn.execute(
            "select count(*) from workspace_members where workspace_id = %s and member_id = %s;",
            (row[0], world["admin"]["id"]),
        ).fetchone()[0]
    assert row[1] == datetime(2026, 1, 1, 0, 0, 0, 123456, tzinfo=UTC)
    assert members == 1
    listed = admin_api.get("/api/instances/workspaces/").json()
    assert "ghost" not in {w["slug"] for w in listed["results"]}


def test_create_workspace_deleted_at_null(admin_api, world):
    res = admin_api.post(
        "/api/instances/workspaces/",
        json={"name": "Live Ws", "slug": "live-ws", "deleted_at": None},
    )
    assert res.status_code == 201
    assert res.json()["deleted_at"] is None
    with world["db"].connect() as conn:
        row = conn.execute(
            "select deleted_at from workspaces where slug = 'live-ws';"
        ).fetchone()[0]
    assert row is None
    listed = admin_api.get("/api/instances/workspaces/").json()
    assert "live-ws" in {w["slug"] for w in listed["results"]}


def test_create_workspace_deleted_at_invalid(admin_api, world):
    res = admin_api.post(
        "/api/instances/workspaces/",
        json={"name": "Bad", "slug": "bad", "deleted_at": "not-a-date"},
    )
    assert res.status_code == 400
    assert res.json() == {"deleted_at": [INVALID_MESSAGE]}
    with world["db"].connect() as conn:
        workspaces = conn.execute("select count(*) from workspaces;").fetchone()[0]
        members = conn.execute("select count(*) from workspace_members;").fetchone()[0]
    assert workspaces == 0
    assert members == 0


def test_create_workspace_deleted_at_error_order(admin_api):
    # A truthy non-string name passes the view pre-checks and fails as a
    # serializer error after deleted_at, in DRF model-field order.
    res = admin_api.post(
        "/api/instances/workspaces/",
        json={"name": ["x"], "slug": "ok-slug", "deleted_at": "bogus"},
    )
    assert res.status_code == 400
    assert list(res.json().keys()) == ["deleted_at", "name"]
