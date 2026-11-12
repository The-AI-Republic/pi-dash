"""Per-test organisation fixture for the D-21 oracle suite.

Each test gets an isolated workspace + two projects with four token-holding
users: a workspace + project admin, a project member, a project guest, and an
outsider with no memberships. A second workspace (owned by a fifth user)
backs the cross-workspace isolation cases. Slugs and intake names carry a
random tag so reruns against the same database never collide.

Authentication is per-user ``X-Api-Key`` tokens (api-v1 has no session auth).
``World.request(method, path, role)`` attaches the role's token; ``role=None``
sends no credentials (the anonymous case).
"""

import uuid

import httpx
import pytest

from _harness import db, http, seed

from . import seed_d21


DUMMY_PASSWORD = "pbkdf2_sha256$600000$ct79contracttestseed$x8yU2Vv9Q0mN6KjH4gF3dS1aZqWeRtYuI="


class World:
    """IDs, API tokens and a role-keyed request helper for one org."""

    def __init__(self, conn, tag: str):
        self.conn = conn
        self.tag = tag
        self.tokens: dict[str | None, str | None] = {}
        self._clients: list[httpx.Client] = []

        self.admin = self._user("admin")
        self.member = self._user("member")
        self.guest = self._user("guest")
        self.outsider = self._user("outsider")
        self.other_admin = self._user("other")

        self.workspace = seed.create_workspace(
            conn, slug=f"ws-{tag}", name=f"WS {tag}", owner_id=self.admin["id"]
        )
        self.project = seed.create_project(
            conn,
            workspace_id=self.workspace["id"],
            identifier=f"D{tag}".upper(),
            name=f"Proj {tag}",
            created_by_id=self.admin["id"],
        )
        # Second project in the same workspace; only the admin belongs to it.
        self.project2 = seed.create_project(
            conn,
            workspace_id=self.workspace["id"],
            identifier=f"E{tag}".upper(),
            name=f"Proj2 {tag}",
            created_by_id=self.admin["id"],
        )
        # Second workspace, owned by an unrelated admin.
        self.workspace2 = seed.create_workspace(
            conn, slug=f"ws2-{tag}", name=f"WS2 {tag}", owner_id=self.other_admin["id"]
        )
        self.intake = seed_d21.create_intake(
            conn,
            workspace_id=self.workspace["id"],
            project_id=self.project["id"],
            created_by_id=self.admin["id"],
            name=f"Main {tag}",
        )
        self.intake2 = seed_d21.create_intake(
            conn,
            workspace_id=self.workspace["id"],
            project_id=self.project2["id"],
            created_by_id=self.admin["id"],
            name=f"Second {tag}",
        )
        for user, wrole, prole in (
            (self.admin, seed.ADMIN, seed.ADMIN),
            (self.member, seed.MEMBER, seed.MEMBER),
            (self.guest, seed.GUEST, seed.GUEST),
        ):
            seed.add_workspace_member(
                conn, workspace_id=self.workspace["id"], user_id=user["id"], role=wrole
            )
            seed.add_project_member(
                conn,
                project_id=self.project["id"],
                workspace_id=self.workspace["id"],
                user_id=user["id"],
                role=prole,
            )
        seed.add_project_member(
            conn,
            project_id=self.project2["id"],
            workspace_id=self.workspace["id"],
            user_id=self.admin["id"],
            role=seed.ADMIN,
        )
        seed.add_workspace_member(
            conn, workspace_id=self.workspace2["id"], user_id=self.other_admin["id"], role=seed.ADMIN
        )
        for role, user in (
            ("admin", self.admin),
            ("member", self.member),
            ("guest", self.guest),
            ("outsider", self.outsider),
            ("other_admin", self.other_admin),
        ):
            self.tokens[role] = seed.create_api_token(
                conn, user_id=user["id"], workspace_id=self.workspace["id"]
            )
        self.tokens[None] = None

    def _user(self, kind: str) -> dict:
        email = f"{kind}-{self.tag}@ct.example.com"
        return seed.create_user(
            self.conn, email=email, username=f"{kind}-{self.tag}", password_field=DUMMY_PASSWORD
        )

    def client(self) -> httpx.Client:
        c = http.make_client()
        self._clients.append(c)
        return c

    def request(self, method: str, path: str, role: str | None, **kw):
        headers = dict(kw.pop("headers", {}) or {})
        if role is not None:
            headers["X-Api-Key"] = self.tokens[role]
        return self.client().request(method, path, headers=headers, **kw)

    def create_intake_issue(self, role: str = "admin", name: str = "intake item", **issue_fields):
        """Create an intake-issue through the API; return the response JSON."""
        payload = {"issue": {"name": name, **issue_fields}}
        r = self.request(
            "POST",
            f"/api/v1/workspaces/{self.workspace['slug']}/projects/{self.project['id']}/intake-issues/",
            role,
            json=payload,
        )
        assert r.status_code == 201, r.text[:500]
        return r.json()

    def close(self):
        for c in self._clients:
            c.close()


@pytest.fixture(scope="session")
def pg():
    conn = db.connect()
    yield conn
    conn.close()


def make_world(conn) -> World:
    return World(conn, uuid.uuid4().hex[:10])


@pytest.fixture()
def org(pg):
    world = make_world(pg)
    yield world
    world.close()
