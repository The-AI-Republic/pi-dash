"""Fixtures for the D-25 (project + states + estimates) contract suite.

Every test seeds its own workspace (uuid-suffixed slugs), so tests never
share rows. Projects under test are created through the API itself, which
exercises the real creation path (default states, triage state, membership).
"""

import os
import sys
import uuid

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from _harness import seed
from _harness.db import connect
from _harness.http import authed_client, base_url, login


@pytest.fixture(scope="session")
def _env():
    for var in ("DATABASE_URL", "BASE_URL", "CONTRACT_SECRET_KEY"):
        if not os.environ.get(var):
            raise RuntimeError(
                f"contract-tests require {var} to be set "
                "(see rust-api/contract-tests/README.md)"
            )
    return {
        "base_url": os.environ["BASE_URL"],
        "secret": os.environ["CONTRACT_SECRET_KEY"],
    }


@pytest.fixture()
def db():
    conn = connect()
    try:
        yield conn
    finally:
        conn.close()


class World:
    """Per-test factory: users, workspaces, memberships, API projects."""

    def __init__(self, conn, base_url, secret):
        self.conn = conn
        self.base_url = base_url
        self.secret = secret

    def user(self, name="ctu"):
        return seed.user(self.conn, name)

    def workspace(self, owner, slug="ctws"):
        ws = seed.workspace(self.conn, slug, owner["id"])
        return ws

    def ws_member(self, ws, user, role):
        return seed.workspace_member(self.conn, ws["id"], user["id"], role=role)

    def client(self, user):
        return authed_client(self.base_url, login(self.conn, user, self.secret))

    def project(self, client, slug, name="World Project", identifier=None):
        tag = uuid.uuid4().hex[:8].upper()
        body = {"name": "%s %s" % (name, tag),
                "identifier": identifier or ("W%s" % tag)[:10]}
        resp = client.post(f"/api/workspaces/{slug}/projects/", json=body)
        assert resp.status_code == 201, resp.text[:500]
        return resp.json()

    def full_stack(self, ws_role=20, proj_role=20):
        """Admin-owned workspace + API project + authed client.

        Returns (client, user, workspace, project). The creator is project
        admin; proj_role adjusts a second member when needed via add_member.
        """
        user = self.user()
        ws = self.workspace(user)
        self.ws_member(ws, user, ws_role)
        client = self.client(user)
        project = self.project(client, ws["slug"])
        if proj_role != 20:
            with self.conn.cursor() as cur:
                cur.execute(
                    'UPDATE project_members SET role=%s WHERE project_id=%s AND member_id=%s',
                    (proj_role, str(project["id"]), str(user["id"])),
                )
            self.conn.commit()
        return client, user, ws, project

    def add_member(self, ws, project, ws_role=15, proj_role=15, name="member"):
        user = self.user(name)
        self.ws_member(ws, user, ws_role)
        seed.project_member(
            self.conn, project["id"], ws["id"], user["id"], role=proj_role)
        return user, self.client(user)


@pytest.fixture()
def world(db, _env):
    return World(db, _env["base_url"], _env["secret"])


def assert_keys(body: dict, expected: list, label: str = ""):
    assert isinstance(body, dict), f"{label}: expected object, got {body!r}"[:300]
    assert sorted(body.keys()) == sorted(expected), (
        f"{label}: keys {sorted(body.keys())} != {expected}")
