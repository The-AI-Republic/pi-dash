"""Domain fixtures: one isolated tenant (user + workspace + project + intake)
per test, seeded straight into Postgres, talking to the live backend over
HTTP with a real session cookie.

Harness generation: first written against the first-generation _harness
(db.conn/create_user/auth.login); rebased onto the Seed-based baseline,
keeping the seeded rows identical. Intake rows are built here because the
domain needs a default intake plus explicit guest scoping the generic
factories don't carry.
"""

import uuid

import pytest

from _harness.auth import login_session
from _harness.config import get_settings
from _harness.db import LazyDatabase
from _harness.http import anonymous_client, api_client
from _harness.seed import ADMIN, GUEST, Seeder, SeedTracker

__all__ = ["ADMIN", "GUEST", "anonymous_client", "intake_urls"]


@pytest.fixture(scope="session")
def settings():
    return get_settings()


def _login(base_url, *, email, password):
    client = api_client(base_url)
    return login_session(client, email=email, password=password)


def _build_tenant(seeder, base_url, *, role=ADMIN):
    """Seed one workspace + project + ADMIN/MEMBER/GUEST owner + default intake."""
    seeder.ensure_instance()
    user = seeder.create_user()
    workspace = seeder.create_workspace(user["id"])
    seeder.create_workspace_member(workspace["id"], user["id"], role=role)
    project = seeder.create_project(workspace["id"])
    seeder.create_project_member(
        workspace["id"], project["id"], user["id"], role=role
    )
    seeder.create_state(workspace["id"], project["id"])
    intake = seeder.create_intake(
        workspace["id"], project["id"], is_default=True
    )
    return {
        "user": user, "workspace": workspace, "project": project,
        "intake": intake, "role": role, "seeder": seeder,
        "client": _login(base_url, email=user["email"], password=user["password"]),
    }


def _close(tenant):
    tenant["client"].close()


@pytest.fixture(scope="session")
def admin(settings):
    """Session tenant for the shape tests (one login, shared by all of them).

    Each login costs anon-throttle budget (AuthenticationThrottle: 30/min
    per IP shared by every suite), so shape tests share this tenant and
    only the access tests mint tenants.
    """
    database = LazyDatabase(settings.database_url)
    seeder = Seeder(database, SeedTracker(database), tag=uuid.uuid4().hex[:10])
    tenant = _build_tenant(seeder, settings.base_url, role=ADMIN)
    tenant["_database"] = database
    yield tenant
    _close(tenant)
    database.close()


@pytest.fixture()
def make_tenant(settings):
    """Fresh isolated tenant per test.

    Rows accumulate in the suite-private database (no per-test cleanup):
    every assertion is scoped to its own tenant's workspace/project, so
    other tenants' rows — seeded or created through the API — never leak
    across. This mirrors the original reset-once design.
    """
    created = []
    databases = []

    def _make(*, role=ADMIN):
        database = LazyDatabase(settings.database_url)
        seeder = Seeder(database, SeedTracker(database), tag=uuid.uuid4().hex[:10])
        tenant = _build_tenant(seeder, settings.base_url, role=role)
        tenant["_database"] = database
        created.append(tenant)
        databases.append(database)
        return tenant

    yield _make

    for tenant in created:
        _close(tenant)
    for database in databases:
        database.close()


def intake_urls(tenant: dict) -> dict:
    base = (
        f"/api/workspaces/{tenant['workspace']['slug']}"
        f"/projects/{tenant['project']['id']}"
    )
    return {
        "intakes": f"{base}/intakes/",
        "inboxes": f"{base}/inboxes/",
        "intake_issues": f"{base}/intake-issues/",
        "inbox_issues": f"{base}/inbox-issues/",
        "versions": lambda issue_id: (
            f"{base}/intake-work-items/{issue_id}/description-versions/"
        ),
    }
