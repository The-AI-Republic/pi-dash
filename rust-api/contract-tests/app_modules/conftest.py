"""Fixtures for the app modules contract suite (PIDASHCONV-86, D-28).

Domain: the 13 URL patterns in apps/api/pi_dash/app/urls/module.py —
modules CRUD, module-issue links (both directions + list), module links,
user favorites, user properties, archive/unarchive + archived list/detail.
Same black-box contract as the other suites: httpx against a live server,
raw SQL seeding, no Django imports.
"""

import httpx
import pytest

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from _harness import env
from _harness.seed import Seed


@pytest.fixture(scope="session")
def api():
    with httpx.Client(base_url=env.base_url(), timeout=15) as client:
        yield client


@pytest.fixture()
def db():
    conn = env.connect()
    try:
        yield conn
    finally:
        conn.close()


@pytest.fixture()
def seed(db):
    s = Seed(db)
    yield s
    s.cleanup()


@pytest.fixture()
def secret():
    return env.contract_secret()


@pytest.fixture()
def tenant_a(seed):
    return seed.tenant()


@pytest.fixture()
def tenant_b(seed):
    return seed.tenant()


def session_headers(seed, user, password, secret):
    cookies = seed.session_cookie(user["id"], password, secret)
    return {"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())}


@pytest.fixture()
def auth_a(seed, tenant_a, secret):
    return session_headers(seed, tenant_a["user"], tenant_a["user"]["password"], secret)


@pytest.fixture()
def auth_b(seed, tenant_b, secret):
    return session_headers(seed, tenant_b["user"], tenant_b["user"]["password"], secret)


@pytest.fixture()
def project_a(seed, tenant_a):
    """A project in tenant A's workspace, owned by tenant A's user."""
    project_id = seed.project(tenant_a["workspace"]["id"])
    seed.project_member(
        project_id,
        tenant_a["workspace"]["id"],
        tenant_a["user"]["id"],
        role=20,
    )
    return project_id


@pytest.fixture()
def module_a(seed, tenant_a, project_a):
    """A seeded module in tenant A's project."""
    return seed.module(tenant_a["workspace"]["id"], project_a)


@pytest.fixture()
def issue_a(seed, tenant_a, project_a):
    """A seeded issue in tenant A's project."""
    return seed.issue(tenant_a["workspace"]["id"], project_a)


def modules_url(tenant, project_id):
    return f"/api/workspaces/{tenant['workspace']['slug']}/projects/{project_id}/modules/"


def module_url(tenant, project_id, pk):
    return f"{modules_url(tenant, project_id)}{pk}/"


# POST create / GET list / PATCH partial_update all render the annotated
# .values() dict (ModuleViewSet.create/partial_update/list).
MODULE_ROW_KEYS = {
    "id", "workspace_id", "project_id",
    "name", "description", "description_text", "description_html",
    "start_date", "target_date", "status", "lead_id", "view_props",
    "sort_order", "external_source", "external_id", "logo_props",
    "created_at", "updated_at",
    "is_favorite", "completed_issues", "cancelled_issues", "started_issues",
    "unstarted_issues", "backlog_issues", "total_issues",
    "completed_estimate_points", "total_estimate_points", "member_ids",
}

# GET retrieve renders ModuleDetailSerializer plus distribution blocks.
MODULE_DETAIL_KEYS = MODULE_ROW_KEYS | {
    "archived_at", "link_module", "sub_issues",
    "backlog_estimate_points", "unstarted_estimate_points",
    "started_estimate_points", "cancelled_estimate_points",
    "estimate_distribution", "distribution",
}
