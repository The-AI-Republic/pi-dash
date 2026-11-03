"""Per-test organisation fixture for the loop suite plus worker-path helpers.

Each test gets an isolated workspace with four users (workspace admin,
member, guest, and an outsider with no memberships) plus a second workspace
owned by an unrelated user for cross-tenant checks. Slugs carry a random tag
so reruns against the same database never collide.

The world's ``admin`` user is an instance admin of the suite's shared
instance row; every other user is not. Admin routes authenticate with the
``admin-session-id`` cookie, app routes with ``session-id`` (same payload,
different cookie name).

Worker-path helpers publish Celery tasks in wire format (``RABBITMQ_URL``)
and inspect queued messages through the broker management API
(``RABBITMQ_MGMT_URL``) with peek-and-requeue reads that never consume.
"""

from __future__ import annotations

import json
import os
import time
import urllib.parse
import uuid

import httpx
import pytest
from celery import Celery

from _harness import db, http, seed, sessions

from . import seed_loop as seed_l

SCAN_TASK = "pi_dash.bgtasks.loop.scan_due_targets"
FIRE_TASK = "pi_dash.bgtasks.loop.fire_loop_target"
RUN_TURN_TASK = "assistant.run_turn"


def broker_url() -> str:
    try:
        return os.environ["RABBITMQ_URL"]
    except KeyError:
        raise RuntimeError(
            "RABBITMQ_URL is required for the loop worker-path tests "
            "(e.g. amqp://pidash:secret@127.0.0.1:5672/pidash)"
        )


def mgmt_url() -> str:
    try:
        return os.environ["RABBITMQ_MGMT_URL"].rstrip("/")
    except KeyError:
        raise RuntimeError(
            "RABBITMQ_MGMT_URL is required for the loop worker-path tests "
            "(e.g. http://127.0.0.1:15672)"
        )


def worker_queue() -> str:
    return os.environ.get("WORKER_QUEUE", "ct17")


def publish(task: str, args: list | None = None) -> str:
    """Publish one Celery task in wire format to the worker queue."""
    app = Celery(broker=broker_url())
    return str(app.send_task(task, args=args or [], queue=worker_queue()).id)


def queued_messages(queue: str, count: int = 100) -> list[dict]:
    """Peek at queued messages without consuming (ack + requeue)."""
    parsed = urllib.parse.urlparse(broker_url())
    vhost_name = parsed.path.lstrip("/") or "/"
    vhost = urllib.parse.quote(vhost_name, safe="")
    userinfo = parsed
    auth = (urllib.parse.unquote(userinfo.username or "guest"),
            urllib.parse.unquote(userinfo.password or "guest"))
    with httpx.Client(timeout=30.0) as c:
        r = c.post(
            f"{mgmt_url()}/api/queues/{vhost}/{queue}/get",
            auth=auth,
            json={"count": count, "ackmode": "ack_requeue_true", "encoding": "auto"},
        )
        r.raise_for_status()
        return r.json()


def purge_queue(queue: str) -> None:
    """Drop every ready message on ``queue`` (broker is suite-dedicated).

    Eligible-due targets never advance until their fire executes, and nothing
    consumes the default queue, so fan-out/run messages pile up across runs
    while ``queued_tasks`` only peeks at the first 100. Purging before a
    broker assertion keeps the observation window on this test's messages.
    """
    parsed = urllib.parse.urlparse(broker_url())
    vhost_name = parsed.path.lstrip("/") or "/"
    vhost = urllib.parse.quote(vhost_name, safe="")
    userinfo = parsed
    auth = (urllib.parse.unquote(userinfo.username or "guest"),
            urllib.parse.unquote(userinfo.password or "guest"))
    with httpx.Client(timeout=30.0) as c:
        r = c.delete(f"{mgmt_url()}/api/queues/{vhost}/{queue}/contents", auth=auth)
        r.raise_for_status()


def queued_tasks(queue: str, task: str) -> list[dict]:
    """Queued ``(headers, body)`` pairs for one Celery task name."""
    out = []
    for msg in queued_messages(queue):
        headers = (msg.get("properties") or {}).get("headers") or {}
        if headers.get("task") != task:
            continue
        payload = msg.get("payload") or ""
        try:
            body = json.loads(payload)
        except ValueError:
            body = None
        out.append({"headers": headers, "body": body})
    return out


def poll(fn, *, timeout: float = 60.0, interval: float = 1.0):
    """Run ``fn`` until it returns truthy; return its last value."""
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        last = fn()
        if last:
            return last
        time.sleep(interval)
    return last


class World:
    """IDs, cookies and a role-keyed request helper for one org."""

    def __init__(self, conn, tag: str):
        self.conn = conn
        self.tag = tag
        self.cookies: dict[str | None, dict] = {}
        self.admin_cookies: dict[str | None, dict] = {}
        self._clients: list[httpx.Client] = []

        self.admin = self._user("admin")
        self.member = self._user("member")
        self.guest = self._user("guest")
        self.outsider = self._user("outsider")
        self.other_user = self._user("other")

        self.workspace = seed.create_workspace(
            conn, slug=f"loop-ws-{tag}", name=f"Loop WS {tag}",
            owner_id=self.admin["id"],
        )
        self.other_ws = seed.create_workspace(
            conn, slug=f"loop-other-{tag}", name=f"Loop Other {tag}",
            owner_id=self.other_user["id"],
        )
        for user, role in (
            (self.admin, seed.ADMIN),
            (self.member, seed.MEMBER),
            (self.guest, seed.GUEST),
        ):
            seed.add_workspace_member(
                conn, workspace_id=self.workspace["id"],
                user_id=user["id"], role=role,
            )
        seed.add_workspace_member(
            conn, workspace_id=self.other_ws["id"],
            user_id=self.other_user["id"], role=seed.ADMIN,
        )
        for role, user in (
            ("admin", self.admin),
            ("member", self.member),
            ("guest", self.guest),
            ("outsider", self.outsider),
            ("other", self.other_user),
        ):
            self.cookies[role] = sessions.login(conn, user["id"], user["password_field"])
            self.admin_cookies[role] = sessions.login_admin(
                conn, user["id"], user["password_field"]
            )
        self.cookies[None] = {}
        self.admin_cookies[None] = {}

        self.instance = seed_l.ensure_instance(conn)
        seed_l.make_instance_admin(
            conn, instance_id=self.instance["id"], user_id=self.admin["id"]
        )

    def _user(self, kind: str) -> dict:
        email = f"loop-{kind}-{self.tag}@ct.example.com"
        return seed.create_user(
            self.conn,
            email=email,
            username=f"loop-{kind}-{self.tag}",
            password_field=sessions.make_password_hash(f"pw-{self.tag}-{kind}"),
        )

    def client(self, role: str | None, *, admin: bool = False) -> httpx.Client:
        jar = self.admin_cookies if admin else self.cookies
        c = http.make_client(jar[role])
        self._clients.append(c)
        return c

    def request(self, method: str, path: str, role: str | None, **kw):
        admin = kw.pop("admin", False)
        return self.client(role, admin=admin).request(method, path, **kw)

    def app(self, method: str, path: str, role: str | None, **kw):
        return self.request(method, path, role, **kw)

    def adm(self, method: str, path: str, role: str | None, **kw):
        return self.request(method, path, role, admin=True, **kw)

    def close(self):
        for c in self._clients:
            c.close()


@pytest.fixture(scope="session")
def pg():
    conn = db.connect()
    yield conn
    conn.close()


@pytest.fixture()
def org(pg):
    world = World(pg, uuid.uuid4().hex[:10])
    yield world
    world.close()
