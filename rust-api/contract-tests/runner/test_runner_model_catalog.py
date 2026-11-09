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

import pytest

from _harness.auth import login_session
from _harness.http import api_client

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


def test_public_catalog_requires_auth(anon_client):
    res = anon_client.get(WEB_CATALOG)
    assert res.status_code == 401
    assert res.json() == {"detail": "Authentication credentials were not provided."}


def test_public_catalog_grouped_enabled_only(settings, seeder, db, user_client):
    # Seed one enabled + one disabled row; only enabled appears.
    _seed_row(db, agent="codex", model="gpt-5.5", label="GPT-5.5", efforts=["low"], sort_order=0)
    _seed_row(db, agent="codex", model="gpt-5.4", label="GPT-5.4", efforts=[], sort_order=1, enabled=False)
    _seed_row(db, agent="claude-code", model="claude-opus-4-8", label="Opus 4.8", preselected=True)
    try:
        res = user_client.get(WEB_CATALOG)
        assert res.status_code == 200, res.text
        agents = res.json()["agents"]
        assert set(agents.keys()) == AGENTS
        assert [r["model"] for r in agents["codex"]] == ["gpt-5.5"]
        assert agents["grok"] == []
        for row in agents["codex"] + agents["claude-code"]:
            assert ROW_KEYS <= set(row)
            assert row["is_enabled"] is True
        claude = agents["claude-code"][0]
        assert claude["is_preselected"] is True
    finally:
        db.execute("DELETE FROM runner_model_catalog WHERE agent IN ('codex', 'claude-code')")


def test_admin_crud_preselect_clearing(settings, seeder, db):
    # Admin session via the instance-admin login (admin cookie).
    instance = db.make_instance()
    admin = db.make_user("catalog-admin@example.com")
    db.make_admin(instance["id"], admin["id"])
    secret = seeder.settings.secret_key if hasattr(seeder, "settings") else None
    # Mint an admin session directly through the DB helper used by the
    # license suite; the runner suite boots with the same `db` fixture.
    from _harness.settings import secret_key

    admin_key = db.mint_admin_session(admin, secret_key())
    from _harness.http import admin_client

    with admin_client(admin_key) as client:
        # Plain user cannot list.
        plain = db.make_user("plain@example.com")
        plain_key = db.mint_admin_session(plain, secret_key())
        with admin_client(plain_key) as plain_client:
            assert plain_client.get(ADMIN_LIST).status_code == 403

        r1 = client.post(ADMIN_LIST, json={"agent": "codex", "model": "gpt-5.5", "label": "GPT-5.5", "reasoning_efforts": ["low"]})
        assert r1.status_code == 201, r1.text
        id1 = r1.json()["id"]
        assert r1.json()["reasoning_efforts"] == ["low"]

        r2 = client.post(ADMIN_LIST, json={"agent": "codex", "model": "gpt-5.6-sol", "label": "GPT-5.6 Sol", "is_preselected": True, "sort_order": 1})
        assert r2.status_code == 201, r2.text
        id2 = r2.json()["id"]

        patched = client.patch(f"{ADMIN_LIST}{id1}/", json={"is_preselected": True})
        assert patched.status_code == 200, patched.text
        assert patched.json()["is_preselected"] is True
        listed = client.get(ADMIN_LIST)
        assert listed.status_code == 200, listed.text
        preselected = [r for r in listed.json() if r["is_preselected"]]
        assert len(preselected) == 1
        assert preselected[0]["id"] == id1

        # Validation: unknown agent → 400 agent, bad effort → 400, duplicate → 400.
        bad_agent = client.post(ADMIN_LIST, json={"agent": "nope", "model": "m", "label": "L"})
        assert bad_agent.status_code == 400
        assert "agent" in bad_agent.json()
        bad_effort = client.post(ADMIN_LIST, json={"agent": "codex", "model": "m2", "label": "L", "reasoning_efforts": ["ultra"]})
        assert bad_effort.status_code == 400
        assert "reasoning_efforts" in bad_effort.json()
        dup = client.post(ADMIN_LIST, json={"agent": "codex", "model": "gpt-5.5", "label": "Dup"})
        assert dup.status_code == 400

        deleted = client.delete(f"{ADMIN_LIST}{id2}/")
        assert deleted.status_code == 204, deleted.text
        missing = client.delete(f"{ADMIN_LIST}{id2}/")
        assert missing.status_code == 404
        assert missing.json() == {"error": "not found"}
    # Cleanup (the per-test reset wipes the world, but remove ours eagerly).
    db.execute("DELETE FROM runner_model_catalog WHERE agent = 'codex' AND model IN ('gpt-5.5', 'gpt-5.6-sol')")


def test_admin_anonymous_401(anon_client):
    assert anon_client.get(ADMIN_LIST).status_code == 401
    assert anon_client.post(ADMIN_LIST, json={"agent": "codex", "model": "m", "label": "L"}).status_code == 401
