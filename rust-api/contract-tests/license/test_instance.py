# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""GET / PATCH /api/instances/ — public console snapshot + admin update."""

import uuid
from datetime import datetime, timezone

from _harness import admin_client
from _harness.settings import secret_key as _secret

CONFIG_KEYS = {
    "enable_signup",
    "is_workspace_creation_disabled",
    "is_google_enabled",
    "is_github_enabled",
    "is_gitlab_enabled",
    "is_gitea_enabled",
    "is_magic_login_enabled",
    "is_email_password_enabled",
    "github_app_name",
    "slack_client_id",
    "posthog_api_key",
    "posthog_host",
    "has_llm_configured",
    "file_size_limit",
    "is_smtp_configured",
    "admin_base_url",
    "space_base_url",
    "app_base_url",
    "instance_changelog_url",
    "is_self_managed",
}


def test_get_without_instance_returns_setup_flag(anon_api):
    res = anon_api.get("/api/instances/")
    assert res.status_code == 200
    assert res.json() == {"is_activated": False, "is_setup_done": False}


def test_get_shape_with_instance(anon_api, world):
    res = anon_api.get("/api/instances/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == {"config", "instance"}
    assert set(body["config"].keys()) == CONFIG_KEYS
    assert isinstance(body["config"]["file_size_limit"], float)
    # NOTE: the no-instance branch returns is_activated, but the view rebuilds
    # `data` from scratch before responding, so the instance branch never
    # carries is_activated. The port must reproduce the absence.
    assert "is_activated" not in body["instance"]
    assert body["instance"]["workspaces_exist"] is False
    assert body["instance"]["instance_name"] == world["instance"]["name"]


INSTANCE_KEYS = {
    "created_at",
    "created_by",
    "current_version",
    "deleted_at",
    "domain",
    "edition",
    "id",
    "instance_id",
    "instance_name",
    "is_current_version_deprecated",
    "is_setup_done",
    "is_signup_screen_visited",
    "is_support_required",
    "is_telemetry_enabled",
    "is_test",
    "is_verified",
    "last_checked_at",
    "latest_version",
    "namespace",
    "updated_at",
    "updated_by",
    "whitelist_emails",
    "workspaces_exist",
}


def test_instance_payload_key_set(anon_api, world):
    body = anon_api.get("/api/instances/").json()
    assert set(body["instance"].keys()) == INSTANCE_KEYS


def test_get_marks_workspaces_exist(anon_api, world):
    world["db"].make_workspace("Acme", "acme", world["admin"]["id"])
    body = anon_api.get("/api/instances/").json()
    assert body["instance"]["workspaces_exist"] is True


def test_patch_updates_name(admin_api, world):
    res = admin_api.patch("/api/instances/", json={"instance_name": "Renamed"})
    assert res.status_code == 200
    assert res.json()["instance_name"] == "Renamed"
    with world["db"].connect() as conn:
        name = conn.execute("select instance_name from instances;").fetchone()[0]
    assert name == "Renamed"


def test_patch_ignores_read_only_fields(admin_api, world):
    with world["db"].connect() as conn:
        conn.execute("update instances set is_setup_done = false;")
        conn.commit()
    res = admin_api.patch(
        "/api/instances/",
        json={"instance_name": "Kept", "is_setup_done": True, "email": "x@y.zz"},
    )
    assert res.status_code == 200
    assert res.json()["instance_name"] == "Kept"
    with world["db"].connect() as conn:
        row = conn.execute("select is_setup_done from instances;").fetchone()
    assert row[0] is False


def test_patch_rejects_overlong_name(admin_api):
    res = admin_api.patch("/api/instances/", json={"instance_name": "n" * 300})
    assert res.status_code == 400


def test_patch_without_instance_denies_before_write(db):
    # Empty instances table (fresh install): InstanceAdminPermission denies
    # (filter on the non-nullable instance FK matches nothing) before the
    # handler runs, so the create path is unreachable via HTTP on both
    # backends — verified live against Django: 403, nothing written.
    user = db.make_user("admin@example.com")
    with admin_client(db.mint_admin_session(user, _secret())) as client:
        res = client.patch("/api/instances/", json={"edition": "x"})
    assert res.status_code == 403
    assert res.content == b'{"detail":"You do not have permission to perform this action."}'
    with db.connect() as conn:
        count = conn.execute("select count(*) from instances;").fetchone()[0]
    assert count == 0


def _make_soft_deleted_instance(db, instance_id):
    iid = str(uuid.uuid4())
    now = datetime.now(timezone.utc)
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                """insert into instances (created_at, updated_at, deleted_at, id, instance_name,
                    instance_id, current_version, last_checked_at,
                    is_telemetry_enabled, is_support_required, is_setup_done,
                    is_signup_screen_visited, is_verified, domain, edition,
                    is_test, is_current_version_deprecated)
                   values (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)""",
                (
                    now, now, now, iid, "Deleted Instance", instance_id,
                    "1.0.0", now, True, True, True, False, False, "",
                    "PI_DASH_COMMUNITY", False, False,
                ),
            )
        conn.commit()
    return iid


def test_patch_instance_id_colliding_soft_deleted_row_is_invalid_payload(admin_api, world):
    # instance_id is a FULL unique: the live-scoped pre-checks (DRF
    # UniqueValidator / reject_duplicate_instance_id) pass, then the write
    # collides with the soft-deleted row. Django maps that IntegrityError
    # to 400 {"error": "The payload is not valid"}; Rust must do the same.
    taken = "soft-deleted-collision-1"
    _make_soft_deleted_instance(world["db"], taken)
    with world["db"].connect() as conn:
        before = conn.execute(
            "select instance_id from instances where deleted_at is null;"
        ).fetchone()[0]
    res = admin_api.patch("/api/instances/", json={"instance_id": taken})
    assert res.status_code == 400
    assert res.content == b'{"error":"The payload is not valid"}'
    with world["db"].connect() as conn:
        row = conn.execute(
            "select instance_id from instances where deleted_at is null;"
        ).fetchone()
        count = conn.execute("select count(*) from instances;").fetchone()[0]
    assert row[0] == before
    assert count == 2
