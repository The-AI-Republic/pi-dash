# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Runner LLM model catalog fixtures over a live server.

Every test seeds Postgres directly and talks HTTP only. Users and the
instance are wiped before each test (``Database.reset()``); the catalog
table (Rust-owned, unknown to the reset) is restored to its seeded
27-row state the same way, so tests never see each other's rows.

On Django the catalog table does not exist (new-system-only feature,
recorded in the differences ledger); the restore is then a no-op and
the endpoint tests answer Django's own 404.
"""

import psycopg
import pytest

from _harness import Database, admin_client, anon_client, user_client
from _harness.settings import database_url, secret_key

# (agent, model, label, efforts, sort_order, is_preselected) — mirrors
# crates/api/src/runner_models/schema.rs SEED (and catalog.ts on day one).
SEED = [
    ("claude-code", "claude-fable-5", "Fable 5", [], 0, False),
    ("claude-code", "claude-opus-5-5", "Opus 5.5", [], 1, False),
    ("claude-code", "claude-opus-5", "Opus 5", [], 2, False),
    ("claude-code", "claude-opus-4-8", "Opus 4.8", [], 3, True),
    ("claude-code", "claude-sonnet-4-6", "Sonnet 4.6", [], 4, False),
    ("claude-code", "claude-sonnet-4-6[1m]", "Sonnet 4.6 (1M context)", [], 5, False),
    ("codex", "gpt-6-astra", "GPT-6 Astra", ["low", "medium", "high", "xhigh"], 0, False),
    ("codex", "gpt-5.6-sol", "GPT-5.6 Sol", ["low", "medium", "high", "xhigh"], 1, False),
    ("codex", "gpt-5.6-terra", "GPT-5.6 Terra", ["low", "medium", "high", "xhigh"], 2, False),
    ("codex", "gpt-5.6-luna", "GPT-5.6 Luna", ["low", "medium", "high", "xhigh"], 3, False),
    ("codex", "gpt-5.5", "GPT-5.5", ["low", "medium", "high", "xhigh"], 4, False),
    ("codex", "gpt-5.4", "GPT-5.4", ["low", "medium", "high", "xhigh"], 5, False),
    ("codex", "gpt-5.4-mini", "GPT-5.4 Mini", ["low", "medium", "high", "xhigh"], 6, False),
    ("cursor-agent", "auto", "Auto", [], 0, False),
    ("cursor-agent", "composer-2.5", "Composer 2.5", [], 1, False),
    ("cursor-agent", "gpt-5.5-high", "GPT-5.5 (High)", [], 2, False),
    ("cursor-agent", "gpt-5.5-extra-high", "GPT-5.5 (Extra High)", [], 3, False),
    ("cursor-agent", "gpt-5.4-medium", "GPT-5.4", [], 4, False),
    ("cursor-agent", "gpt-5.4-mini-medium", "GPT-5.4 Mini", [], 5, False),
    ("cursor-agent", "claude-opus-4-8-high", "Claude Opus 4.8", [], 6, False),
    ("cursor-agent", "claude-opus-4-8-thinking-high", "Claude Opus 4.8 (Thinking)", [], 7, False),
    ("cursor-agent", "claude-4.6-sonnet-medium", "Claude Sonnet 4.6", [], 8, False),
    ("cursor-agent", "claude-4.6-sonnet-medium-thinking", "Claude Sonnet 4.6 (Thinking)", [], 9, False),
    ("cursor-agent", "gemini-3.1-pro", "Gemini 3.1 Pro", [], 10, False),
    ("cursor-agent", "grok-4.3", "Grok 4.3", [], 11, False),
    ("grok", "grok-4.3", "Grok 4.3", [], 0, False),
    ("grok", "grok-code-fast-1", "Grok Code Fast 1", [], 1, False),
]


def _restore_seed(db: Database) -> None:
    """Restore the catalog to its seeded 27-row state (Rust only)."""
    with db.connect() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT to_regclass('rust_runner_model_catalog')")
            if cur.fetchone()[0] is None:
                return
            cur.execute("DELETE FROM rust_runner_model_catalog")
            for agent, model, label, efforts, sort_order, preselected in SEED:
                cur.execute(
                    """INSERT INTO rust_runner_model_catalog
                       (id, agent, model, label, reasoning_efforts,
                        sort_order, is_enabled, is_preselected,
                        created_at, updated_at)
                       VALUES (gen_random_uuid(), %s, %s, %s, %s,
                               %s, true, %s, now(), now())""",
                    (agent, model, label, efforts, sort_order, preselected),
                )
        conn.commit()


@pytest.fixture(scope="session")
def db():
    return Database(database_url())


@pytest.fixture(autouse=True)
def clean_db(db):
    db.reset()
    _restore_seed(db)
    yield


@pytest.fixture
def world(db):
    admin = db.make_user("admin@example.com")
    member = db.make_user("member@example.com", first_name="Plain", last_name="Member")
    # A second non-admin user: minting a session deletes that user's prior
    # sessions, so one user cannot hold both cookie types at once.
    member2 = db.make_user("member2@example.com", first_name="Second", last_name="Member")
    instance = db.make_instance()
    db.make_admin(instance["id"], admin["id"])
    secret = secret_key()
    return {
        "db": db,
        "admin": admin,
        "member": member,
        "member2": member2,
        "instance": instance,
        "admin_key": db.mint_admin_session(admin, secret),
        "member_admin_key": db.mint_admin_session(member2, secret),
        "member_key": db.mint_user_session(member, secret),
    }


@pytest.fixture
def admin_api(world):
    with admin_client(world["admin_key"]) as client:
        yield client


@pytest.fixture
def member_admin_api(world):
    """Admin-cookie client for a user WITHOUT an instance-admin row."""
    with admin_client(world["member_admin_key"]) as client:
        yield client


@pytest.fixture
def member_api(world):
    """App-cookie (session-id) client for the signed-in public read."""
    with user_client(world["member_key"]) as client:
        yield client


@pytest.fixture
def anon_api():
    with anon_client() as client:
        yield client
