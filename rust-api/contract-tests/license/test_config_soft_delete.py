# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Soft-deleted instance_configurations rows are ignored (Django parity).

Django reads this table through the default scoped manager, which excludes
rows with deleted_at set and falls back to the default. Every config read
through PgConfigStore must do the same. (PIDASHCONV-1096)
"""


def _soft_delete(db, key):
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "UPDATE instance_configurations SET deleted_at = now() WHERE key = %s;",
                (key,),
            )
        conn.commit()


def test_soft_deleted_flag_falls_back_to_default(anon_api, world):
    db = world["db"]
    # Baseline with no row: env default "0" -> False.
    assert anon_api.get("/api/instances/").json()["config"]["enable_signup"] is False
    # Live row wins.
    db.make_config("ENABLE_SIGNUP", "1", category="general")
    assert anon_api.get("/api/instances/").json()["config"]["enable_signup"] is True
    # Soft-deleted row is ignored: back to the default.
    _soft_delete(db, "ENABLE_SIGNUP")
    assert anon_api.get("/api/instances/").json()["config"]["enable_signup"] is False


def test_soft_deleted_email_host_falls_back_to_default(anon_api, world):
    db = world["db"]
    # Baseline with no row: the server env sets EMAIL_HOST=localhost, so the
    # fallback is truthy and SMTP reads as configured.
    assert anon_api.get("/api/instances/").json()["config"]["is_smtp_configured"] is True
    # Live empty row (as disable-email writes) disables SMTP.
    db.make_config("EMAIL_HOST", "", category="email")
    assert anon_api.get("/api/instances/").json()["config"]["is_smtp_configured"] is False
    # Soft-deleted row is ignored: back to the localhost fallback.
    _soft_delete(db, "EMAIL_HOST")
    assert anon_api.get("/api/instances/").json()["config"]["is_smtp_configured"] is True
