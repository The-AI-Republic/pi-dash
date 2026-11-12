"""Shared fixtures for the app-pages contract suite (PIDASHCONV-88, D-30).

Covers ``app/urls/page.py`` (11 paths): summary, list/create, retrieve/
patch/delete, favorite create/destroy, archive/unarchive, lock/unlock,
access, description retrieve/patch, versions list/detail, duplicate.

Every test seeds its own world (unique tag per test) straight into Postgres
and deletes only its own rows afterwards. HTTP goes through httpx; sessions
come from the real sign-in endpoint (see ``_harness.auth``).

Role convention (``app/permissions/base.py`` ROLE): 20 ADMIN, 15 MEMBER,
5 GUEST. The world owner is an ADMIN of the project; extra users cover the
member, guest, outsider (no membership) and second-workspace cases.
"""

from __future__ import annotations

import uuid

import pytest

from _harness.auth import login_session
from _harness.config import get_settings
from _harness.db import LazyDatabase
from _harness.http import anonymous_client, api_client
from _harness.seed import Seeder, SeedTracker


@pytest.fixture(scope="session")
def settings():
    return get_settings()


@pytest.fixture()
def db(settings):
    # LazyDatabase is the shared thin psycopg wrapper (lazy connect so
    # ``pytest --collect-only`` works with no database around).
    database = LazyDatabase(settings.database_url)
    yield database
    database.close()


@pytest.fixture()
def seeder(db):
    tracker = SeedTracker(db)
    seeder = Seeder(db, tracker, tag=uuid.uuid4().hex[:10])
    yield seeder
    tracker.cleanup()


def _project_world(seeder, *, page_name="Contract page"):
    """One workspace + project + ADMIN owner + linked public page."""
    seeder.ensure_instance()
    owner = seeder.create_user()
    workspace = seeder.create_workspace(owner["id"])
    project = seeder.create_project(workspace["id"])
    seeder.create_project_member(workspace["id"], project["id"], owner["id"], role=20)
    page = seeder.create_page(workspace["id"], owner["id"], name=page_name)
    seeder.link_page_project(workspace["id"], project["id"], page["id"], by_id=owner["id"])
    return {
        "owner": owner,
        "workspace": workspace,
        "project": project,
        "page": page,
    }


@pytest.fixture()
def world(seeder):
    return _project_world(seeder)


@pytest.fixture()
def world2(seeder):
    """A second, fully separate workspace/project/page for isolation cases."""
    return _project_world(seeder, page_name="Other workspace page")


def _member_user(seeder, world, *, role):
    user = seeder.create_user()
    seeder.create_project_member(world["workspace"]["id"], world["project"]["id"], user["id"], role=role)
    return user


@pytest.fixture()
def member_user(seeder, world):
    return _member_user(seeder, world, role=15)


@pytest.fixture()
def guest_user(seeder, world):
    return _member_user(seeder, world, role=5)


@pytest.fixture()
def outsider_user(seeder):
    """Authenticated but a member of nothing: the denied-permission case."""
    seeder.ensure_instance()
    return seeder.create_user()


def _logged_in(settings, user):
    client = api_client(settings.base_url)
    login_session(client, email=user["email"], password=user["password"])
    return client


@pytest.fixture()
def user_client(settings, world):
    # No `with` block: _logged_in already issues requests, and httpx >= 0.28
    # raises on __enter__ after implicit open. Close explicitly instead.
    client = _logged_in(settings, world["owner"])
    yield client
    client.close()


@pytest.fixture()
def member_client(settings, member_user):
    client = _logged_in(settings, member_user)
    yield client
    client.close()


@pytest.fixture()
def guest_client(settings, guest_user):
    client = _logged_in(settings, guest_user)
    yield client
    client.close()


@pytest.fixture()
def outsider_client(settings, outsider_user):
    client = _logged_in(settings, outsider_user)
    yield client
    client.close()


@pytest.fixture()
def anon_client(settings):
    with anonymous_client(settings.base_url) as client:
        yield client


def pages_base(world):
    return f"/api/workspaces/{world['workspace']['slug']}/projects/{world['project']['id']}"


def page_url(world, page_id=None):
    base = f"{pages_base(world)}/pages"
    return f"{base}/{page_id}/" if page_id else f"{base}/"
