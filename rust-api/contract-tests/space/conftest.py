"""Shared fixtures for the space public-API contract suite (PIDASHCONV-16).

Every test seeds its own world (unique tag per test) straight into Postgres
and deletes only its own rows afterwards. HTTP goes through httpx; sessions
come from the real sign-in endpoint (see ``_harness.auth``).
"""

from __future__ import annotations

import uuid

import pytest

from _harness.auth import login_session
from _harness.config import get_settings
from _harness.db import LazyDatabase as Database
from _harness.http import anonymous_client, api_client
from _harness.seed import Seeder, SeedTracker


@pytest.fixture(scope="session")
def settings():
    return get_settings()


@pytest.fixture()
def db(settings):
    database = Database(settings.database_url)
    yield database
    database.close()


@pytest.fixture()
def seeder(db):
    tracker = SeedTracker(db)
    seeder = Seeder(db, tracker, tag=uuid.uuid4().hex[:10])
    yield seeder
    tracker.cleanup()


@pytest.fixture()
def world(seeder):
    """A complete public-board world: user, workspace, project, board with
    every flag on, plus one row per listable entity."""
    seeder.ensure_instance()
    owner = seeder.create_user()
    workspace = seeder.create_workspace(owner["id"])
    project = seeder.create_project(workspace["id"])
    intake = seeder.create_intake(workspace["id"], project["id"])
    board = seeder.create_board(workspace["id"], project["id"], intake_id=intake["id"])
    state = seeder.create_state(workspace["id"], project["id"])
    issue = seeder.create_issue(workspace["id"], project["id"], state["id"])
    comment = seeder.create_comment(workspace["id"], project["id"], issue["id"], owner["id"])
    reaction = seeder.create_issue_reaction(workspace["id"], project["id"], issue["id"], owner["id"])
    comment_reaction = seeder.create_comment_reaction(
        workspace["id"], project["id"], comment["id"], owner["id"]
    )
    vote = seeder.create_vote(workspace["id"], project["id"], issue["id"], owner["id"])
    cycle = seeder.create_cycle(workspace["id"], project["id"], owner["id"])
    module = seeder.create_module(workspace["id"], project["id"])
    label = seeder.create_label(workspace["id"], project["id"])
    asset = seeder.create_asset(workspace["id"], project["id"], owner["id"])
    return {
        "owner": owner,
        "workspace": workspace,
        "project": project,
        "intake": intake,
        "board": board,
        "anchor": board["anchor"],
        "state": state,
        "issue": issue,
        "comment": comment,
        "reaction": reaction,
        "comment_reaction": comment_reaction,
        "vote": vote,
        "cycle": cycle,
        "module": module,
        "label": label,
        "asset": asset,
    }


@pytest.fixture()
def other_user(seeder):
    seeder.ensure_instance()
    return seeder.create_user()


@pytest.fixture()
def user_client(settings, world):
    with api_client(settings.base_url) as client:
        login_session(client, email=world["owner"]["email"], password=world["owner"]["password"])
        yield client


@pytest.fixture()
def other_client(settings, other_user):
    with api_client(settings.base_url) as client:
        login_session(client, email=other_user["email"], password=other_user["password"])
        yield client


@pytest.fixture()
def anon_client(settings):
    with anonymous_client(settings.base_url) as client:
        yield client
