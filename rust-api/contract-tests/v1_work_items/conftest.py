"""Fixtures for the api-v1 work-items contract suite (PIDASHCONV-76).

Every test seeds its own world (unique tag per test) straight into Postgres
and deletes only its own rows afterwards. HTTP goes through httpx with the
``X-Api-Key`` header: api-v1 authenticates API keys (``APIKeyAuthentication``
against ``api_tokens``), not session cookies.

The suite runs against a live server (Django today, the Rust server through
the proxy tomorrow) selected by ``BASE_URL``; the database it seeds is
selected by ``DATABASE_URL``. The server also needs Redis (throttle cache)
and a Celery broker (write endpoints enqueue activity tasks) — see
``rust-api/contract-tests/README.md``.
"""

from __future__ import annotations

import uuid

import httpx
import pytest

from _harness.config import get_settings
from _harness.db import LazyDatabase
from _harness.seed import Seeder, SeedTracker

ENVELOPE_KEYS = {
    "grouped_by",
    "sub_grouped_by",
    "total_count",
    "next_cursor",
    "prev_cursor",
    "next_page_results",
    "prev_page_results",
    "count",
    "total_pages",
    "total_results",
    "extra_stats",
    "results",
}


@pytest.fixture(scope="session")
def settings():
    return get_settings()


@pytest.fixture()
def db(settings):
    database = LazyDatabase(settings.database_url)
    yield database
    database.close()


@pytest.fixture()
def seeder(db):
    tracker = SeedTracker(db)
    seeder = Seeder(db, tracker, tag=uuid.uuid4().hex[:10])
    yield seeder
    tracker.cleanup()


def build_world(seeder, base_url, *, member_role=20):
    """Seed user + workspace + project + token; create one issue via the API."""
    owner = seeder.create_user()
    workspace = seeder.create_workspace(owner["id"])
    seeder.create_workspace_member(workspace["id"], owner["id"], role=20)
    project = seeder.create_project(workspace["id"])
    seeder.create_project_member(workspace["id"], project["id"], owner["id"], role=member_role)
    token = seeder.create_api_token(owner["id"], workspace["id"])
    headers = {"X-Api-Key": token["token"]}
    with httpx.Client(base_url=base_url, timeout=30, headers=headers) as client:
        response = client.post(
            f"/api/v1/workspaces/{workspace['slug']}/projects/{project['id']}/work-items/",
            json={"name": "Contract issue", "priority": "none"},
        )
        assert response.status_code == 201, response.text
        issue = response.json()
    return {
        "owner": owner,
        "workspace": workspace,
        "project": project,
        "token": token,
        "headers": headers,
        "issue": issue,
    }


@pytest.fixture()
def world(seeder, settings):
    return build_world(seeder, settings.base_url)


@pytest.fixture()
def api(settings, world):
    with httpx.Client(base_url=settings.base_url, timeout=30, headers=world["headers"]) as client:
        yield client


@pytest.fixture()
def anon(settings):
    with httpx.Client(base_url=settings.base_url, timeout=30) as client:
        yield client


@pytest.fixture()
def guest_client(seeder, settings, world):
    """A GUEST-role (5) project member: SAFE reads pass, writes 403."""
    guest = seeder.create_user()
    seeder.create_workspace_member(world["workspace"]["id"], guest["id"], role=5)
    seeder.create_project_member(
        world["workspace"]["id"], world["project"]["id"], guest["id"], role=5
    )
    token = seeder.create_api_token(guest["id"], world["workspace"]["id"])
    with httpx.Client(
        base_url=settings.base_url, timeout=30, headers={"X-Api-Key": token["token"]}
    ) as client:
        yield client


@pytest.fixture()
def other_world(seeder, settings):
    """A second tenant: cross-tenant reads must not leak rows."""
    return build_world(seeder, settings.base_url)


@pytest.fixture()
def other_api(settings, other_world):
    with httpx.Client(
        base_url=settings.base_url, timeout=30, headers=other_world["headers"]
    ) as client:
        yield client


def item_url(world, *parts):
    base = f"/api/v1/workspaces/{world['workspace']['slug']}/projects/{world['project']['id']}"
    return base + "".join(f"/{p}" for p in parts) + "/"


def issue_url(world, issue_id=None):
    url = item_url(world, "work-items").rstrip("/")
    return f"{url}/" if issue_id is None else f"{url}/{issue_id}/"
