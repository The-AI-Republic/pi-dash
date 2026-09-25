"""Fixtures for the app-notifications contract suite (D-34, PIDASHCONV-92).

Topology (seeded once per session under a unique run tag):

- workspace A (``ctx.slug_a``): owner (ADMIN), member (MEMBER), both (MEMBER)
- workspace B (``ctx.slug_b``): b_member (MEMBER), both (MEMBER)
- outsider: authenticated, member of nothing

Per-test notifications go through ``mknotif`` (function scope, tracked and
deleted at teardown) so mutating tests cannot leak state into each other.
"""
import uuid

import pytest

import sys, os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from _harness import auth, config, factory  # noqa: E402


def _tag() -> str:
    return f"ctn{uuid.uuid4().hex[:8]}"


class Ctx:
    def __init__(self):
        self.tag = _tag()
        self.base_url = config.base_url()


@pytest.fixture(scope="session")
def ctx():
    c = Ctx()
    tag = c.tag
    owner = factory.create_user(f"{tag}-owner@example.com")
    member = factory.create_user(f"{tag}-member@example.com")
    both = factory.create_user(f"{tag}-both@example.com")
    b_member = factory.create_user(f"{tag}-bmember@example.com")
    outsider = factory.create_user(f"{tag}-outsider@example.com")
    guest = factory.create_user(f"{tag}-guest@example.com")
    ws_a = factory.create_workspace(f"{tag}-a", owner["id"], name="CTN A")
    ws_b = factory.create_workspace(f"{tag}-b", owner["id"], name="CTN B")
    factory.add_member(ws_a["id"], owner["id"], factory.ROLE_ADMIN)
    factory.add_member(ws_a["id"], member["id"], factory.ROLE_MEMBER)
    factory.add_member(ws_a["id"], both["id"], factory.ROLE_MEMBER)
    factory.add_member(ws_a["id"], guest["id"], factory.ROLE_GUEST)
    factory.add_member(ws_b["id"], b_member["id"], factory.ROLE_MEMBER)
    factory.add_member(ws_b["id"], both["id"], factory.ROLE_MEMBER)
    c.owner, c.member, c.both, c.b_member, c.outsider, c.guest = (
        owner, member, both, b_member, outsider, guest,
    )
    c.ws_a, c.ws_b = ws_a, ws_b
    c.slug_a, c.slug_b = ws_a["slug"], ws_b["slug"]
    c.cookies = {
        u["email"]: auth.login_session_cookie(c.base_url, u["email"], u["password"])
        for u in (owner, member, both, b_member, outsider, guest)
    }
    yield c
    factory.cleanup_run(tag)


def _client(ctx, user):
    return auth.api_client(ctx.base_url, ctx.cookies[user["email"]])


@pytest.fixture(scope="session")
def api_owner(ctx):
    with _client(ctx, ctx.owner) as c:
        yield c


@pytest.fixture(scope="session")
def api_member(ctx):
    with _client(ctx, ctx.member) as c:
        yield c


@pytest.fixture(scope="session")
def api_both(ctx):
    with _client(ctx, ctx.both) as c:
        yield c


@pytest.fixture(scope="session")
def api_bmember(ctx):
    with _client(ctx, ctx.b_member) as c:
        yield c


@pytest.fixture(scope="session")
def api_guest(ctx):
    with _client(ctx, ctx.guest) as c:
        yield c


@pytest.fixture(scope="session")
def api_outsider(ctx):
    with _client(ctx, ctx.outsider) as c:
        yield c


@pytest.fixture(scope="session")
def api_anon(ctx):
    with auth.api_client(ctx.base_url, None) as c:
        yield c


@pytest.fixture()
def mknotif(ctx):
    """Create notifications in workspace A for the owner; delete afterwards."""
    ids = []

    def make(**kw):
        kw.setdefault("workspace_id", ctx.ws_a["id"])
        kw.setdefault("receiver_id", ctx.owner["id"])
        n = factory.create_notification(**kw)
        ids.append(n["id"])
        return n

    yield make
    for nid in ids:
        factory.delete_notification(nid)
