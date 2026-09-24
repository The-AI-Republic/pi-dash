"""Environment configuration for contract suites."""

import os


def base_url() -> str:
    return os.environ["BASE_URL"].rstrip("/")


def database_url() -> str:
    return os.environ["DATABASE_URL"]


def secret_key() -> str:
    """Django SECRET_KEY of the backend under test (for session minting)."""
    return os.environ["SECRET_KEY"]
