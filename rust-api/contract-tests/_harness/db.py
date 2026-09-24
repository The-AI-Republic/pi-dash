"""Direct-Postgres helpers for domains that seed data for the live backend.

``DATABASE_URL`` comes from the environment and is only required by
suites that actually seed (web_edge needs no DB). Raw-SQL seeding,
snapshots and polling live here (psycopg; no ORM).
"""
from __future__ import annotations

import os
import time
import uuid

import psycopg
import pytest


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


def connect(database_url: str) -> psycopg.Connection:
    return psycopg.connect(database_url, autocommit=True)


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
