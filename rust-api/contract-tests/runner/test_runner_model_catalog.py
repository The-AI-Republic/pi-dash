# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Runner LLM model catalog contract (PDASHOSS01-364).

Pins the Django-owned contract for both surfaces, word for word:

* `GET /api/runners/model-catalog/` — any signed-in user. Enabled rows
  grouped `{"agents": {agent: [row,...]}}` with every agent key present,
  ordered by `sort_order`, `created_at`. Anonymous → 401.
* `GET/POST /api/instances/runner-models/` and
  `PATCH/DELETE /api/instances/runner-models/<id>/` — instance admins
  only. Authenticated non-admin → 403, anonymous → 401. Validation is a
  400 field-error map; duplicate `(agent, model)` → 400; preselect
  clears the agent's other rows atomically.

Runs unchanged against Django and the Rust port (`BASE_URL` /
`DATABASE_URL` / `SECRET_KEY`); both backends must satisfy the same
absolute assertions.
"""

from __future__ import annotations

import json
import uuid
from datetime import datetime, timedelta, timezone

import pytest

from _harness import signing

pytestmark = pytest.mark.contract

WEB_CATALOG = "/api/runners/model-catalog/"
ADMIN_LIST = "/api/instances/runner-models/"
AGENTS = {"claude-code", "codex", "cursor-agent", "open-claw", "grok", "muse-code"}
ROW_KEYS = {
    "id", "agent", "model", "label", "reasoning_efforts", "sort_order",
    "is_enabled", "is_preselected", "created_at", "updated_at",
}


def _seed_row(db, *, agent, model, label, efforts=None, sort_order=0, enabled=True, preselected=False):
    efforts = efforts if efforts is not None else []
    import json

    db.execute(
        "INSERT INTO runner_model_catalog "
        "(id, agent, model, label, reasoning_efforts, sort_order, is_enabled, is_preselected, created_at, updated_at) "
        "VALUES (gen_random_uuid(), %s, %s, %s, %s::jsonb, %s, %s, %s, now(), now())",
        (agent, model, label, json.dumps(efforts), sort_order, enabled, preselected),
    )


def _fetch_id(db, agent, model):
    row = db.fetchone(
        "SELECT id FROM runner_model_catalog WHERE agent = %s AND model = %s",
        (agent, model),
    )
    assert row is not None, f"seed {agent}:{model} missing"
    return str(row[0] if isinstance(row, (list, tuple)) else row["id"])


def _mint_session(db, user_id, secret):
    """Forge a sessions row (mirrors the license suite's mint_admin_session)."""
    row = db.fetchone("SELECT password FROM users WHERE id = %s", (user_id,))
    assert row is not None, f"user {user_id} missing"
    password_hash = row["password"] if isinstance(row, dict) else row[0]
    payload = signing.session_payload(user_id, password_hash, secret)
    data = signing.encode_session(payload, secret)
    key = signing.new_session_key()
    expires = datetime.now(timezone.utc) + timedelta(seconds=3600)
    db.execute("DELETE FROM sessions WHERE user_id = %s", (user_id,))
    db.execute(
        "INSERT INTO sessions (session_key, session_data, expire_date, user_id, device_info) "
        "VALUES (%s, %s, %s, %s, %s::jsonb)",
        (key, data, expires, user_id, json.dumps(payload["device_info"])),
    )
    return key


def _make_instance_admin(db, seeder, *, email):
    """Create a user granted instance-admin on the suite instance."""
    seeder.ensure_instance()
    user = seeder.create_user(email=email)
    instance_id = db.fetchval("SELECT id FROM instances ORDER BY created_at LIMIT 1")
    db.execute(
        "INSERT INTO instance_admins "
        "(id, created_at, updated_at, role, is_verified, instance_id, user_id) "
        "VALUES (%s, now(), now(), 20, true, %s, %s)",
        (str(uuid.uuid4()), instance_id, user["id"]),
    )
    return user


def test_public_catalog_requires_auth(anon_client):
    res = anon_client.get(WEB_CATALOG)
    assert res.status_code == 401
    assert res.json() == {"detail": "Authentication credentials were not provided."}


def test_public_catalog_grouped_enabled_only(settings, seeder, db, user_client):
    # Seed one enabled + one disabled row under muse-code (no migration
    # seed rows there); only the enabled one appears. Slugs are probe-unique
    # so the test runs against a migrated DB that already holds the seed.
    _seed_row(db, agent="muse-code", model="zz-364-enabled", label="Probe Enabled", efforts=[], sort_order=0, preselected=True)
    _seed_row(db, agent="muse-code", model="zz-364-disabled", label="Probe Disabled", efforts=[], sort_order=1, enabled=False)
    try:
        res = user_client.get(WEB_CATALOG)
        assert res.status_code == 200, res.text
        agents = res.json()["agents"]
        assert set(agents.keys()) == AGENTS
        assert [r["model"] for r in agents["muse-code"]] == ["zz-364-enabled"]
        for rows in agents.values():
            for row in rows:
                assert row["is_enabled"] is True
        probe = agents["muse-code"][0]
        assert ROW_KEYS <= set(probe)
        assert probe["is_preselected"] is True
    finally:
        db.execute(
            "DELETE FROM runner_model_catalog WHERE model IN ('zz-364-enabled', 'zz-364-disabled')"
        )


def test_admin_crud_preselect_clearing(settings, seeder, db):
    # Admin session via the instance-admin login (admin cookie).
    from _harness.http import admin_client

    admin = _make_instance_admin(db, seeder, email="catalog-admin@example.com")
    admin_key = _mint_session(db, admin["id"], settings.secret_key)
    try:
        with admin_client(admin_key) as client:
            # Plain user cannot list.
            plain = seeder.create_user(email="catalog-plain@example.com")
            plain_key = _mint_session(db, plain["id"], settings.secret_key)
            try:
                with admin_client(plain_key) as plain_client:
                    assert plain_client.get(ADMIN_LIST).status_code == 403
            finally:
                db.execute("DELETE FROM sessions WHERE user_id = %s", (plain["id"],))

            r1 = client.post(ADMIN_LIST, json={"agent": "codex", "model": "zz-364-gpt-9", "label": "GPT-9", "reasoning_efforts": ["low"]})
            assert r1.status_code == 201, r1.text
            id1 = r1.json()["id"]
            assert r1.json()["reasoning_efforts"] == ["low"]

            r2 = client.post(ADMIN_LIST, json={"agent": "codex", "model": "zz-364-gpt-9b", "label": "GPT-9b", "is_preselected": True, "sort_order": 1})
            assert r2.status_code == 201, r2.text
            id2 = r2.json()["id"]

            patched = client.patch(f"{ADMIN_LIST}{id1}/", json={"is_preselected": True})
            assert patched.status_code == 200, patched.text
            assert patched.json()["is_preselected"] is True
            listed = client.get(ADMIN_LIST)
            assert listed.status_code == 200, listed.text
            # Only one preselected codex row (seed rows for other agents may
            # carry their own preselected flags, so scope to this agent).
            preselected = [r for r in listed.json() if r["agent"] == "codex" and r["is_preselected"]]
            assert len(preselected) == 1
            assert preselected[0]["id"] == id1

            # Validation: unknown agent → 400 agent, bad effort → 400, duplicate → 400.
            bad_agent = client.post(ADMIN_LIST, json={"agent": "nope", "model": "m", "label": "L"})
            assert bad_agent.status_code == 400
            assert "agent" in bad_agent.json()
            bad_effort = client.post(ADMIN_LIST, json={"agent": "codex", "model": "m2", "label": "L", "reasoning_efforts": ["ultra"]})
            assert bad_effort.status_code == 400
            assert "reasoning_efforts" in bad_effort.json()
            dup = client.post(ADMIN_LIST, json={"agent": "codex", "model": "zz-364-gpt-9", "label": "Dup"})
            assert dup.status_code == 400

            deleted = client.delete(f"{ADMIN_LIST}{id2}/")
            assert deleted.status_code == 204, deleted.text
            missing = client.delete(f"{ADMIN_LIST}{id2}/")
            assert missing.status_code == 404
            assert missing.json() == {"error": "not found"}
    finally:
        # Remove only probe rows; never touch the migration seed.
        db.execute("DELETE FROM runner_model_catalog WHERE model IN ('zz-364-gpt-9', 'zz-364-gpt-9b')")
        db.execute("DELETE FROM instance_admins WHERE user_id = %s", (admin["id"],))
        db.execute("DELETE FROM sessions WHERE user_id = %s", (admin["id"],))


def test_admin_anonymous_401(anon_client):
    assert anon_client.get(ADMIN_LIST).status_code == 401
    assert anon_client.post(ADMIN_LIST, json={"agent": "codex", "model": "m", "label": "L"}).status_code == 401
