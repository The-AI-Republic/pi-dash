# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Instance configurations: list/patch shapes, email disable, email check."""

EMAIL_KEYS = ["EMAIL_HOST", "EMAIL_HOST_USER", "EMAIL_HOST_PASSWORD", "ENABLE_SMTP", "EMAIL_PORT", "EMAIL_FROM"]

CONFIG_ITEM_KEYS = {"id", "key", "value", "category", "is_encrypted", "source", "is_managed",
                    "created_at", "updated_at", "created_by", "updated_by"}


def _seed_mail(db):
    db.make_config("EMAIL_HOST", "smtp.example.com", category="email")
    db.make_config("EMAIL_HOST_USER", "mailer", category="email")
    db.make_config("EMAIL_HOST_PASSWORD", "s3cret", category="email")
    db.make_config("ENABLE_SMTP", "1", category="email")
    db.make_config("EMAIL_PORT", "587", category="email")
    db.make_config("EMAIL_FROM", "noreply@example.com", category="email")


def test_list_configurations_shape(admin_api, world):
    _seed_mail(world["db"])
    res = admin_api.get("/api/instances/configurations/")
    assert res.status_code == 200
    by_key = {item["key"]: item for item in res.json()}
    assert set(by_key) == set(EMAIL_KEYS)
    host = by_key["EMAIL_HOST"]
    assert CONFIG_ITEM_KEYS <= set(host.keys())
    assert host["value"] == "smtp.example.com"
    assert host["is_encrypted"] is False


def test_patch_updates_value(admin_api, world):
    world["db"].make_config("CONTRACT_PLAIN", "old", category="general")
    res = admin_api.patch("/api/instances/configurations/", json={"CONTRACT_PLAIN": "new"})
    assert res.status_code == 200
    (item,) = res.json()
    assert item["key"] == "CONTRACT_PLAIN"
    assert item["value"] == "new"
    with world["db"].connect() as conn:
        stored = conn.execute(
            "select value from instance_configurations where key = 'CONTRACT_PLAIN';"
        ).fetchone()[0]
    assert stored == "new"


def test_patch_strips_surrounding_whitespace(admin_api, world):
    world["db"].make_config("CONTRACT_PADDED", "old", category="general")
    res = admin_api.patch("/api/instances/configurations/", json={"CONTRACT_PADDED": "  padded  "})
    assert res.status_code == 200
    assert res.json()[0]["value"] == "padded"


def test_patch_unknown_keys_are_ignored(admin_api):
    res = admin_api.patch("/api/instances/configurations/", json={"NO_SUCH_KEY": "x"})
    assert res.status_code == 200
    assert res.json() == []


def test_patch_encrypts_at_rest_but_returns_plaintext(admin_api, world):
    world["db"].make_config("CONTRACT_SECRET", "old", category="general", is_encrypted=True)
    res = admin_api.patch("/api/instances/configurations/", json={"CONTRACT_SECRET": "fresh"})
    assert res.status_code == 200
    assert res.json()[0]["value"] == "fresh"
    with world["db"].connect() as conn:
        stored = conn.execute(
            "select value from instance_configurations where key = 'CONTRACT_SECRET';"
        ).fetchone()[0]
    assert stored != "fresh"
    listed = admin_api.get("/api/instances/configurations/").json()
    assert [i for i in listed if i["key"] == "CONTRACT_SECRET"][0]["value"] == "fresh"


def test_disable_email_feature(admin_api, world):
    _seed_mail(world["db"])
    res = admin_api.delete("/api/instances/configurations/disable-email-feature/")
    assert res.status_code == 200
    with world["db"].connect() as conn:
        rows = dict(
            conn.execute("select key, value from instance_configurations;").fetchall()
        )
    assert rows["ENABLE_SMTP"] == "0"
    for key in ("EMAIL_HOST", "EMAIL_HOST_USER", "EMAIL_HOST_PASSWORD", "EMAIL_PORT", "EMAIL_FROM"):
        assert rows[key] == ""


def test_email_check_requires_receiver(admin_api):
    res = admin_api.post("/api/instances/email-credentials-check/", json={})
    assert res.status_code == 400
    assert res.json() == {"error": "Receiver email is required"}
