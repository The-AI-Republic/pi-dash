"""Contract tests: integration datetimes render Django isoformat (PIDASHCONV-1464).

Python ``datetime.isoformat()`` prints exactly six fraction digits whenever
microseconds are nonzero. The Rust slice used chrono ``AutoSi``, which trims
trailing zeros (``.120``) and prints nine digits for nanosecond in-memory
values. These tests seed microsecond-exact stamps (``.120000``) and assert
the six-digit rendering through the API on both backends.
"""

from __future__ import annotations

import pytest

from .conftest import proj_url, ws_url

pytestmark = pytest.mark.contract

STAMP_SQL = "2026-10-22 10:00:05.120000+00"
STAMP_JSON = "2026-10-22T10:00:05.120000+00:00"


def test_git_account_verified_at_keeps_six_digits(admin, world, seeder):
    account = seeder.create_git_provider_account(world["workspace"]["id"])
    seeder.db.execute(
        "UPDATE git_provider_accounts SET verified_at = %s WHERE id = %s",
        (STAMP_SQL, account["id"]),
    )
    response = admin.get(ws_url(world, "integrations", "git", "accounts", account["id"]))
    assert response.status_code == 200
    assert response.json()["verified_at"] == STAMP_JSON


def test_git_binding_last_synced_at_keeps_six_digits(admin, world, seeder):
    account = seeder.create_git_provider_account(world["workspace"]["id"])
    seeder.create_git_repository_binding(
        world["workspace"]["id"],
        world["project"]["id"],
        account["id"],
        world["owner"]["id"],
        last_synced_at=STAMP_SQL,
    )
    response = admin.get(proj_url(world, "repository"))
    assert response.status_code == 200
    body = response.json()
    assert body["bound"] is True
    assert body["last_synced_at"] == STAMP_JSON


def test_github_app_status_verified_at_keeps_six_digits(admin, world, seeder):
    wi = seeder.create_workspace_integration(world["workspace"]["id"], world["owner"]["id"])
    installation = seeder.create_github_app_installation(wi["id"])
    seeder.db.execute(
        "UPDATE github_app_installations SET verified_at = %s, last_checked_at = %s WHERE id = %s",
        (STAMP_SQL, STAMP_SQL, installation["id"]),
    )
    body = admin.get("/api/users/me/integrations/github/app/").json()
    entry = next(item for item in body["workspaces"] if item["slug"] == world["workspace"]["slug"])
    assert entry["github_app"]["connected"] is True
    assert entry["github_app"]["verified_at"] == STAMP_JSON
    assert entry["github_app"]["last_checked_at"] == STAMP_JSON
