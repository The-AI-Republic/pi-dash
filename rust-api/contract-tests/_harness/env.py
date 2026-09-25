"""Environment + psycopg plumbing shared by every contract suite."""

import os

import psycopg


def base_url() -> str:
    return os.environ.get("BASE_URL", "http://127.0.0.1:8000").rstrip("/")


def database_url() -> str:
    try:
        return os.environ["DATABASE_URL"]
    except KeyError:
        raise AssertionError(
            "DATABASE_URL is required (psycopg conninfo, e.g. "
            "'host=/tmp dbname=pidash_contract_80 user=irichard')"
        )


def contract_secret() -> str:
    """SECRET_KEY of the Django server under test.

    Needed only to forge session cookies / machine-token hashes when
    seeding auth state straight into Postgres. Must equal the server's
    SECRET_KEY or the forged rows will not authenticate.
    """
    try:
        return os.environ["CONTRACT_SECRET_KEY"]
    except KeyError:
        raise AssertionError(
            "CONTRACT_SECRET_KEY is required (must match the server's SECRET_KEY)"
        )


def web_base() -> str:
    """Public web base the server uses to build verification URIs."""
    try:
        return os.environ["CONTRACT_WEB_URL"].rstrip("/")
    except KeyError:
        raise AssertionError(
            "CONTRACT_WEB_URL is required (must match the server's WEB_URL)"
        )


def connect():
    return psycopg.connect(database_url(), autocommit=True)


# -- Module-level constants (PIDASHCONV-81, v1_openapi domain). Same sources
# as the functions above, read once at import; kept alongside (never a fork)
# because the v1_openapi suite reads ``env.BASE_URL`` / ``env.DATABASE_URL``.
#: HTTP root of the backend under test, e.g. http://127.0.0.1:8481
BASE_URL = os.environ.get("BASE_URL", "http://127.0.0.1:8475").rstrip("/")

#: Postgres the backend reads/writes (empty when unset; use require_db).
DATABASE_URL = os.environ.get("DATABASE_URL", "")

#: Celery broker the backend's worker consumes, e.g. redis://localhost:6379/9
CELERY_BROKER_URL = os.environ.get("CELERY_BROKER_URL", "redis://127.0.0.1:6379/9")

#: Upper bound for waiting on worker effects. Generous: CI workers are slow.
OPERATOR_TIMEOUT = float(os.environ.get("CONTRACT_TIMEOUT", "60"))

#: Poll step for worker effects.
POLL_INTERVAL = float(os.environ.get("CONTRACT_POLL", "0.25"))


def require_db() -> str:
    if not DATABASE_URL:
        raise RuntimeError(
            "DATABASE_URL is not set. Boot a backend per "
            "rust-api/contract-tests/README.md and export DATABASE_URL."
        )
    return DATABASE_URL
