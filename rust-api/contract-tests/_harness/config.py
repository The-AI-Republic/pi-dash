"""Environment configuration for contract suites.

Only two variables are required; both point at live infrastructure:

- ``BASE_URL`` — e.g. ``http://127.0.0.1:18013`` (no trailing slash).
- ``DATABASE_URL`` — psycopg-connectable Postgres URL for the same backend,
  used only for seeding/cleanup SQL, never for assertions about internals.

Suites that mint auth sessions additionally use ``SECRET_KEY`` (see
``secret_key`` below).
"""
import os


def base_url() -> str:
    try:
        return os.environ["BASE_URL"].rstrip("/")
    except KeyError:
        raise RuntimeError("BASE_URL is not set (e.g. http://127.0.0.1:18013)") from None


def database_url() -> str:
    try:
        return os.environ["DATABASE_URL"]
    except KeyError:
        raise RuntimeError("DATABASE_URL is not set (psycopg URL for the backend DB)") from None


def secret_key() -> str:
    """Django SECRET_KEY of the backend under test (for session minting)."""
    return os.environ["SECRET_KEY"]
