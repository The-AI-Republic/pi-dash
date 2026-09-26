"""Shared fixtures for the runner daemon-API contract suite (PIDASHCONV-97).

Every test seeds its own world (unique tag per test) straight into Postgres
and deletes only its own rows afterwards. HTTP goes through httpx; runner
access/refresh tokens come from the real enroll flow, machine tokens from the
real ``RunnerCreate`` rotation, sessions from the real sign-in endpoint.

Auth inventory (all under ``/api/v1/runner/``):
- AllowAny: health/, metrics/, runners/enroll/, machine-tokens/ (redeem).
- X-Api-Key (``APIToken`` plaintext or ``mt_`` machine token): runners/
  (create), projects/ (list).
- Bearer JWT (enroll/refresh flow): runner sessions, runs/*, chat/*.
- Bearer ``mt_`` + URL ``runner_id`` (or ``X-Runner-Id``): same set, daemon
  wire format — the installed-runner path.
- Bearer ``mt_`` bound to the URL dev machine: machine sessions + results.
- Session cookie + desktop flag: dev-machines/desktop-enroll/ (the flag is
  only stamped by the cloud overlay, so CE always answers 403 here — pinned).
"""

from __future__ import annotations

import secrets
import uuid

import pytest

from _harness.auth import login_session
from _harness.config import get_settings
from _harness.db import LazyDatabase as Database
from _harness.http import anonymous_client, api_client, api_key_client, bearer_client
from _harness.seed import Seeder, SeedTracker

DAEMON = "/api/v1/runner"


@pytest.fixture(scope="session")
def settings():
    return get_settings()


@pytest.fixture()
def db(settings):
    database = Database(settings.database_url)
    yield database
    database.close()


@pytest.fixture()
def seeder(db, settings):
    tracker = SeedTracker(db)
    seeder = Seeder(db, tracker, tag=uuid.uuid4().hex[:10], secret_key=settings.secret_key)
    yield seeder
    tracker.cleanup()


@pytest.fixture()
def anon_client(settings):
    with anonymous_client(settings.base_url) as client:
        yield client


@pytest.fixture()
def daemon_world(seeder):
    """One member user + workspace + project + default pod + API token."""
    seeder.ensure_instance()
    owner = seeder.create_user()
    workspace = seeder.create_workspace(owner["id"])
    seeder.create_member(workspace["id"], owner["id"])
    project = seeder.create_project(workspace["id"])
    pod = seeder.create_pod(workspace["id"], project["id"], is_default=True)
    api_token = seeder.create_api_token(owner["id"])
    return {
        "owner": owner,
        "workspace": workspace,
        "project": project,
        "pod": pod,
        "api_token": api_token,
    }


@pytest.fixture()
def user_client(settings, daemon_world):
    """Session-cookie client for the world owner (desktop-denied pins)."""
    with api_client(settings.base_url) as client:
        login_session(
            client,
            email=daemon_world["owner"]["email"],
            password=daemon_world["owner"]["password"],
        )
        yield client


@pytest.fixture()
def machine_flow(settings, seeder, daemon_world):
    """CLI path: ``POST runners/`` with X-Api-Key mints runner + machine token.

    Mirrors ``pidash runner add``: the APIToken caller passes a host label and
    gets a rotated ``mt_`` token bound to the new dev machine.
    """
    host_label = f"daemon-{seeder.tag}"[:200]
    with api_key_client(settings.base_url, daemon_world["api_token"]["token"]) as client:
        response = client.post(
            f"{DAEMON}/runners/",
            json={
                "project": daemon_world["project"]["identifier"],
                "workspace_slug": daemon_world["workspace"]["slug"],
                "host_label": host_label,
            },
        )
    assert response.status_code == 201, response.text
    body = response.json()
    assert body["machine_token"].startswith("mt_")
    flow = {
        "runner_id": body["runner_id"],
        "runner_name": body["runner_name"],
        "machine_token": body["machine_token"],
        "host_label": host_label,
    }
    yield flow
    # Teardown rows the endpoint created (untracked by the seeder). Runs and
    # chat sessions seeded against this runner are tracker-owned and go
    # later, so unlink them first (their child rows are already gone via the
    # daemon_run/chat_world finalizers, which run before this one).
    owner_id = daemon_world["owner"]["id"]
    seeder.db.execute("DELETE FROM runner_session WHERE runner_id = %s", (flow["runner_id"],))
    seeder.db.execute(
        "UPDATE agent_run SET runner_id = NULL, pinned_runner_id = NULL "
        "WHERE runner_id = %s OR pinned_runner_id = %s",
        (flow["runner_id"], flow["runner_id"]),
    )
    seeder.db.execute("DELETE FROM agent_chat_session WHERE runner_id = %s", (flow["runner_id"],))
    seeder.db.execute("DELETE FROM machine_token WHERE user_id = %s AND host_label = %s", (owner_id, host_label))
    seeder.db.execute(
        "DELETE FROM runner WHERE id = %s",
        (flow["runner_id"],),
    )
    seeder.db.execute(
        "DELETE FROM dev_machine WHERE owner_id = %s AND host_label = %s", (owner_id, host_label)
    )


@pytest.fixture()
def machine_client(settings, machine_flow):
    """Bearer ``mt_`` client — the installed-daemon wire format.

    Runs/chat URLs carry no runner id, so the runner identity travels in
    the ``X-Runner-Id`` header there (``RunnerAccessTokenAuthentication``
    falls back to it when the URL has no ``runner_id``); session URLs
    prefer their own URL id and ignore the header.
    """
    with bearer_client(
        settings.base_url,
        machine_flow["machine_token"],
        headers={"X-Runner-Id": machine_flow["runner_id"]},
    ) as client:
        yield client


@pytest.fixture()
def enrolled(settings, seeder, daemon_world):
    """Legacy path: seed an unenrolled runner, redeem via ``runners/enroll/``.

    Returns the JWT access + refresh pair the daemon then uses.
    """
    enrollment_raw = f"apd_en_{secrets.token_urlsafe(24)}"
    host_label = f"enroll-{seeder.tag}"[:200]
    seeded = seeder.enroll_runner(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        enrollment_raw,
    )
    with anonymous_client(settings.base_url) as client:
        response = client.post(
            f"{DAEMON}/runners/enroll/",
            json={"enrollment_token": enrollment_raw, "host_label": host_label},
        )
    assert response.status_code == 201, response.text
    body = response.json()
    flow = {
        "runner_id": body["runner_id"],
        "runner_name": seeded["name"],
        "access_token": body["access_token"],
        "refresh_token": body["refresh_token"],
        "host_label": host_label,
        "enroll_body": body,
    }
    yield flow
    seeder.db.execute("DELETE FROM runner_session WHERE runner_id = %s", (flow["runner_id"],))
    seeder.db.execute(
        "DELETE FROM machine_token WHERE user_id = %s AND host_label = %s",
        (daemon_world["owner"]["id"], host_label),
    )
    # The enroll flow binds a dev machine to the seeded runner; unlink then
    # drop it (the seeded runner row itself is removed by the seed tracker).
    seeder.db.execute(
        "UPDATE runner SET dev_machine_id = NULL WHERE id = %s", (flow["runner_id"],)
    )
    seeder.db.execute(
        "DELETE FROM dev_machine WHERE owner_id = %s AND host_label = %s",
        (daemon_world["owner"]["id"], host_label),
    )


@pytest.fixture()
def runner_client(settings, enrolled):
    """Bearer JWT client bound to the enrolled runner."""
    with bearer_client(settings.base_url, enrolled["access_token"]) as client:
        yield client


@pytest.fixture()
def daemon_run(seeder, daemon_world, machine_flow):
    """An ASSIGNED run owned by the flow runner, with child cleanup.

    Run endpoints append events/dedupe/approval rows the seeder never sees;
    the finalizer removes them before the seed tracker drops the run itself
    (the FKs are checked per statement under autocommit).
    """
    run = seeder.create_daemon_run(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        daemon_world["pod"]["id"],
        machine_flow["runner_id"],
        status="assigned",
    )
    yield run
    seeder.db.execute("DELETE FROM run_message_dedupe WHERE run_id = %s", (run["id"],))
    seeder.db.execute("DELETE FROM agent_run_event WHERE agent_run_id = %s", (run["id"],))
    seeder.db.execute("DELETE FROM agent_run_approval WHERE agent_run_id = %s", (run["id"],))


@pytest.fixture()
def chat_world(seeder, daemon_world, machine_flow):
    """An OPEN chat session owned by the flow runner, with child cleanup."""
    session = seeder.create_chat_session(
        daemon_world["workspace"]["id"],
        machine_flow["runner_id"],
        daemon_world["pod"]["id"],
        daemon_world["owner"]["id"],
    )
    message = seeder.create_chat_message(session["id"], role="user", content="hello")
    world = {"session": session, "message": message}
    yield world
    seeder.db.execute(
        "DELETE FROM agent_chat_event WHERE session_id = %s", (session["id"],)
    )
    seeder.db.execute(
        "DELETE FROM chat_message_dedupe WHERE session_id = %s", (session["id"],)
    )
    seeder.db.execute(
        "DELETE FROM agent_chat_approval WHERE session_id = %s", (session["id"],)
    )
    seeder.db.execute(
        "DELETE FROM agent_chat_message WHERE session_id = %s", (session["id"],)
    )
