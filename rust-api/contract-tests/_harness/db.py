"""Direct-Postgres helpers for domains that seed data for the live backend.

``DATABASE_URL`` comes from the environment and is only required by
suites that actually seed (web_edge needs no DB). psycopg is imported
lazily so DB-free domains never pay for it.
"""

import os

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
    psycopg = __import__("psycopg")
    with psycopg.connect(get_database_url(), autocommit=True) as conn:
        yield conn
