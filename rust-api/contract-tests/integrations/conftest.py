"""Shared fixtures: env config + seeded git-sync rows + guaranteed teardown."""
from __future__ import annotations

import os

import pytest

from _harness import db

from . import seed


def _require(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise pytest.skip(f"{name} is not set — point it at the live Django stack")
    return value


@pytest.fixture(scope="session")
def base_url() -> str:
    return _require("BASE_URL")


@pytest.fixture(scope="session")
def database_url() -> str:
    return _require("DATABASE_URL")


@pytest.fixture(scope="session")
def broker_url() -> str:
    return _require("CELERY_BROKER_URL")


@pytest.fixture(scope="session")
def anchor(database_url: str) -> dict:
    """Borrow one workspace + actor + project from the target DB (read-only).

    Seeded sync rows hang off these; the anchor rows themselves are never
    modified. Note git_repository_bindings allows one active binding per
    project, so multi-binding fan-out tests reuse one binding across
    sequential enable/disable phases rather than seeding two at once.
    """
    return seed.find_anchor(database_url)


@pytest.fixture()
def sync_scope(database_url: str, anchor: dict):
    """Yield a tracker that records every seeded row; delete all on teardown."""
    scope = seed.Scope(database_url)
    yield scope
    scope.cleanup()


@pytest.fixture()
def github_binding(database_url: str, anchor: dict, sync_scope) -> dict:
    """One enabled provider-neutral binding with an invalid token.

    The worker's attempt hits api.github.com → 401 → GithubAuthError →
    deterministic error-recording path (no retry, account degraded).
    """
    return seed.github_binding(
        database_url, anchor, sync_scope, token="contract-test-invalid-token"
    )
