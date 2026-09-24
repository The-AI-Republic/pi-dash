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
