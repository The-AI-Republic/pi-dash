"""Per-test organisation fixture for the app-assets suite.

Each test gets an isolated workspace + project with four users: a workspace
+ project admin, a member, a guest, and an outsider with no memberships.
Slugs carry a random tag so reruns against the same database never collide.
"""

import uuid

import httpx
import pytest

from _harness import db, http, seed, sessions


class World:
    """IDs, cookies and a role-keyed request helper for one org."""

    def __init__(self, conn, tag: str):
        self.conn = conn
        self.tag = tag
        self.cookies: dict[str | None, dict] = {}
        self._clients: list[httpx.Client] = []

        self.admin = self._user("admin", seed.ADMIN, seed.ADMIN)
        self.member = self._user("member", seed.MEMBER, seed.MEMBER)
        self.guest = self._user("guest", seed.GUEST, seed.GUEST)
        self.outsider = self._user("outsider", None, None)

        self.workspace = seed.create_workspace(
            conn,
            slug=f"ws-{tag}",
            name=f"WS {tag}",
            owner_id=self.admin["id"],
        )
        self.project = seed.create_project(
            conn,
            workspace_id=self.workspace["id"],
            identifier=f"C{tag}".upper(),
            name=f"Proj {tag}",
            created_by_id=self.admin["id"],
        )
        for user, wrole, prole in (
            (self.admin, seed.ADMIN, seed.ADMIN),
            (self.member, seed.MEMBER, seed.MEMBER),
            (self.guest, seed.GUEST, seed.GUEST),
        ):
            seed.add_workspace_member(
                conn,
                workspace_id=self.workspace["id"],
                user_id=user["id"],
                role=wrole,
            )
            seed.add_project_member(
                conn,
                project_id=self.project["id"],
                workspace_id=self.workspace["id"],
                user_id=user["id"],
                role=prole,
            )
        for role, user in (
            ("admin", self.admin),
            ("member", self.member),
            ("guest", self.guest),
            ("outsider", self.outsider),
        ):
            self.cookies[role] = sessions.login(
                conn, user["id"], user["password_field"]
            )
        self.cookies[None] = {}

    def _user(self, kind: str, wrole: int | None, prole: int | None) -> dict:
        email = f"{kind}-{self.tag}@ct.example.com"
        return seed.create_user(
            self.conn,
            email=email,
            username=f"{kind}-{self.tag}",
            password_field=sessions.make_password_hash(f"pw-{self.tag}"),
        )

    def client(self, role: str | None) -> httpx.Client:
        c = http.make_client(self.cookies[role])
        self._clients.append(c)
        return c

    def request(self, method: str, path: str, role: str | None, **kw):
        return self.client(role).request(method, path, **kw)

    def close(self):
        for c in self._clients:
            c.close()


@pytest.fixture(scope="session")
def pg():
    conn = db.connect()
    yield conn
    conn.close()


def make_world(conn) -> World:
    return make_world_with_tag(conn, uuid.uuid4().hex[:10])


def make_world_with_tag(conn, tag: str) -> World:
    world = World(conn, tag)
    return world


@pytest.fixture()
def org(pg):
    world = make_world(pg)
    yield world
    world.close()
