# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Direct-Postgres helpers for domains that seed data for the live backend.

``DATABASE_URL`` comes from the environment and is only required by
suites that actually seed (web_edge needs no DB). Raw-SQL seeding,
snapshots and polling live here (psycopg; no ORM, no Django imports).
``db_conn`` below imports psycopg lazily so DB-free domains never pay
for it; ``db_cursor`` offers a committing cursor for suites whose tests
never clean up: every seeded row carries a random suffix, so suites are
safe to re-run and to parallelise without cross-test interference.

Suites that seed do so straight into Postgres with plain SQL (no ORM, no
Django imports). Connections are autocommit; each test seeds
uniquely-named rows so no cleanup pass is needed for repeat runs against
the same database.
"""
from __future__ import annotations

import json
import os
from typing import Any, Sequence
import secrets
import time
import uuid
from contextlib import contextmanager
from datetime import datetime, timezone, timedelta
from typing import Iterator

import psycopg
import pytest

from . import signing

from _harness.config import database_url

KNOWN_PASSWORD = "ContractPass123!"


def get_database_url() -> str:
    try:
        return os.environ["DATABASE_URL"]
    except KeyError as exc:
        raise pytest.UsageError(
            "DATABASE_URL is not set "
            "(e.g. DATABASE_URL=postgresql://postgres@localhost/pidash)"
        ) from exc


@pytest.fixture(scope="session")
def db_conn():
    with psycopg.connect(get_database_url(), autocommit=True) as conn:
        yield conn


def connect(database_url: str | None = None) -> psycopg.Connection:
    """Open an autocommit Postgres connection (``DATABASE_URL`` by default)."""
    return psycopg.connect(database_url or get_database_url(), autocommit=True)


def fetchone(database_url: str, sql: str, params: tuple = ()) -> dict | None:
    with connect(database_url) as conn, conn.cursor(row_factory=psycopg.rows.dict_row) as cur:
        cur.execute(sql, params)
        return cur.fetchone()


def fetchall(database_url: str, sql: str, params: tuple = ()) -> list[dict]:
    with connect(database_url) as conn, conn.cursor(row_factory=psycopg.rows.dict_row) as cur:
        cur.execute(sql, params)
        return list(cur.fetchall())


def execute(database_url: str, sql: str, params: tuple = ()) -> None:
    with connect(database_url) as conn, conn.cursor() as cur:
        cur.execute(sql, params)


def new_uuid() -> str:
    return str(uuid.uuid4())


def wait_for(
    database_url: str,
    sql: str,
    params: tuple = (),
    timeout: float = 90.0,
    poll: float = 1.0,
) -> dict | None:
    """Poll until a row matches. Returns the row, or None on timeout.

    The worker executes asynchronously; every effect assertion goes
    through here instead of a fixed sleep.
    """
    deadline = time.time() + timeout
    while time.time() < deadline:
        row = fetchone(database_url, sql, params)
        if row:
            return row
        time.sleep(poll)
    return None

def _now():
    return datetime.now(timezone.utc)


class Database:
    def __init__(self, conninfo):
        self.conninfo = conninfo

    def connect(self):
        return psycopg.connect(self.conninfo)

    def reset(self):
        # Dependency order. `schedulers` rows are signal-created per
        # workspace; a missing table here fails loudly as an FK violation.
        tables = [
            "schedulers",
            "sessions",
            "workspace_members",
            "workspaces",
            "instance_admins",
            "instance_configurations",
            "instances",
            "users",
        ]
        with self.connect() as conn:
            with conn.cursor() as cur:
                for table in tables:
                    cur.execute(f"delete from {table};")
            conn.commit()

    def make_user(self, email, *, password=KNOWN_PASSWORD, first_name="Contract", last_name="User", is_active=True):
        uid = str(uuid.uuid4())
        now = _now()
        with self.connect() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    """insert into users (password, id, username, email, first_name, last_name,
                        avatar, date_joined, created_at, updated_at, last_location,
                        created_location, is_superuser, is_managed, is_password_expired,
                        is_active, is_staff, is_email_verified, is_password_autoset,
                        token, user_timezone, last_login_ip, last_logout_ip,
                        last_login_medium, last_login_uagent, is_bot, display_name,
                        is_email_valid, is_password_reset_required)
                       values (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)
                       returning password""",
                    (
                        signing.make_password_hash(password), uid, f"u-{uid[:8]}", email,
                        first_name, last_name, "", now, now, now, "", "", False, False,
                        False, is_active, False, True, False, secrets.token_hex(32),
                        "UTC", "", "", "email", "", False, f"{first_name} {last_name}",
                        True, False,
                    ),
                )
                password_hash = cur.fetchone()[0]
            conn.commit()
        return {"id": uid, "email": email, "password": password, "password_hash": password_hash}

    def make_instance(self, *, name="Contract Instance", is_setup_done=True):
        iid = str(uuid.uuid4())
        now = _now()
        with self.connect() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    """insert into instances (created_at, updated_at, id, instance_name,
                        instance_id, current_version, last_checked_at,
                        is_telemetry_enabled, is_support_required, is_setup_done,
                        is_signup_screen_visited, is_verified, domain, edition,
                        is_test, is_current_version_deprecated)
                       values (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)""",
                    (
                        now, now, iid, name, f"instance-{iid[:8]}", "1.0.0", now,
                        True, True, is_setup_done, False, False, "", "PI_DASH_COMMUNITY",
                        False, False,
                    ),
                )
            conn.commit()
        return {"id": iid, "name": name}

    def make_admin(self, instance_id, user_id, *, role=20):
        aid = str(uuid.uuid4())
        now = _now()
        with self.connect() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    """insert into instance_admins
                       (created_at, updated_at, id, role, is_verified, instance_id, user_id)
                       values (%s,%s,%s,%s,%s,%s,%s)""",
                    (now, now, aid, role, True, instance_id, user_id),
                )
            conn.commit()
        return {"id": aid}

    def make_config(self, key, value, *, category="general", is_encrypted=False):
        cid = str(uuid.uuid4())
        now = _now()
        with self.connect() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    """insert into instance_configurations
                       (created_at, updated_at, id, key, value, category, is_encrypted)
                       values (%s,%s,%s,%s,%s,%s,%s)""",
                    (now, now, cid, key, value, category, is_encrypted),
                )
            conn.commit()
        return {"id": cid}

    def mint_admin_session(self, user, secret, *, max_age=3600):
        payload = signing.session_payload(user["id"], user["password_hash"], secret)
        data = signing.encode_session(payload, secret)
        key = signing.new_session_key()
        expires = _now() + timedelta(seconds=max_age)
        with self.connect() as conn:
            with conn.cursor() as cur:
                cur.execute("delete from sessions where user_id = %s;", (user["id"],))
                cur.execute(
                    """insert into sessions
                       (session_key, session_data, expire_date, user_id, device_info)
                       values (%s,%s,%s,%s,%s::jsonb)""",
                    (key, data, expires, user["id"], json.dumps(payload["device_info"])),
                )
            conn.commit()
        return key

    def make_workspace(self, name, slug, owner_id, *, timezone="UTC"):
        wid = str(uuid.uuid4())
        now = _now()
        with self.connect() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    """insert into workspaces
                       (created_at, updated_at, id, name, slug, owner_id, timezone, background_color)
                       values (%s,%s,%s,%s,%s,%s,%s,%s)""",
                    (now, now, wid, name, slug, owner_id, timezone, "#ffffff"),
                )
            conn.commit()
        return {"id": wid, "name": name, "slug": slug}

    def make_workspace_member(self, workspace_id, user_id, *, role=20):
        mid = str(uuid.uuid4())
        now = _now()
        with self.connect() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    """insert into workspace_members
                       (created_at, updated_at, id, role, member_id, workspace_id,
                        view_props, default_props, issue_props, is_active,
                        explored_features, getting_started_checklist, tips)
                       values (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)""",
                    (
                        now, now, mid, role, user_id, workspace_id,
                        json.dumps({}), json.dumps({}), json.dumps({}), True,
                        json.dumps({}), json.dumps({}), json.dumps({}),
                    ),
                )
            conn.commit()
        return {"id": mid}

class LazyDatabase:
    """Thin psycopg wrapper. Connections are lazy so ``pytest --collect-only``
    works with no database around; fixtures connect on first use.

    Named apart from ``Database`` (the/reset + make_* helper owned by the
    license suite): same ``(dsn)`` constructor shape, disjoint methods.
    """

    def __init__(self, dsn: str):
        self._dsn = dsn
        self._conn = None

    def connect(self):
        if self._conn is None or self._conn.closed:
            from psycopg.rows import dict_row

            self._conn = psycopg.connect(self._dsn, row_factory=dict_row, autocommit=True)
        return self._conn

    def execute(self, sql: str, params: Sequence[Any] | None = None):
        with self.connect().cursor() as cur:
            cur.execute(sql, params or ())
            return cur

    def fetchone(self, sql: str, params: Sequence[Any] | None = None):
        with self.connect().cursor() as cur:
            cur.execute(sql, params or ())
            return cur.fetchone()

    def fetchall(self, sql: str, params: Sequence[Any] | None = None):
        with self.connect().cursor() as cur:
            cur.execute(sql, params or ())
            return cur.fetchall()

    def fetchval(self, sql: str, params: Sequence[Any] | None = None):
        row = self.fetchone(sql, params)
        if not row:
            return None
        return next(iter(row.values()))

    def close(self):
        if self._conn is not None and not self._conn.closed:
            self._conn.close()
        self._conn = None


@contextmanager
def db_cursor() -> Iterator[psycopg.Cursor]:
    """Yield a cursor on the backend's database; commits on clean exit."""
    with psycopg.connect(database_url()) as conn:
        with conn.cursor() as cur:
            yield cur
        conn.commit()
