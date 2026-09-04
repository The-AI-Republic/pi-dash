# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""MCP tool servers: model, CRUD API, and toolset construction.

The behavioural contract worth protecting here is that tool servers are
*additive*. A user's broken or hostile server configuration must degrade that
server only — never the assistant turn, and never another server.
"""

from __future__ import annotations

import pytest
from rest_framework.test import APIClient

from pi_dash.assistant import crypto
from pi_dash.assistant.models import AssistantMCPServer
from pi_dash.assistant.runtime import mcp as mcp_runtime

pytestmark = pytest.mark.django_db

URL = "/api/users/me/ai-assistant/mcp-servers/"


@pytest.fixture(autouse=True)
def _hermetic_crypto(settings, monkeypatch):
    """Give every test in this file a working crypto backend of its own.

    Without this, the auth-header tests depend on whatever key material the
    ambient environment happens to provide — green where a KMS/Fernet key is
    configured, 503s in a bare CI runner.
    """
    from cryptography.fernet import Fernet

    settings.ASSISTANT_CRYPTO_BACKEND = "fernet"
    settings.ASSISTANT_ENCRYPTION_KEY = Fernet.generate_key().decode()
    monkeypatch.setattr(crypto, "_backend", None)


def client_for(user):
    c = APIClient()
    c.force_authenticate(user=user)
    return c


def make_server(user, **kwargs) -> AssistantMCPServer:
    defaults = {"name": "Tools", "url": "https://tools.example.com/mcp", "is_enabled": True}
    defaults.update(kwargs)
    return AssistantMCPServer.objects.create(user=user, **defaults)


# --------------------------------------------------------------------------- #
# Model
# --------------------------------------------------------------------------- #


def test_tool_prefix_is_slugified_from_the_name(world):
    server = make_server(world.member, name="My Jira Tools!")
    assert server.tool_prefix == "my_jira_tools"


def test_tool_prefix_falls_back_when_the_name_has_no_alphanumerics(world):
    # A prefix must always be non-empty: an empty one would let a server's tool
    # names collide with the built-in tools.
    server = make_server(world.member, name="!!! ???")
    assert server.tool_prefix.startswith("mcp_")
    assert len(server.tool_prefix) > len("mcp_")


def test_same_name_is_rejected_per_user_but_allowed_across_users(world):
    from django.db.utils import IntegrityError

    make_server(world.member, name="Dup")
    with pytest.raises(IntegrityError):
        make_server(world.member, name="Dup")


def test_two_users_may_each_have_a_server_named_the_same(world):
    make_server(world.member, name="Shared")
    # Must not raise: uniqueness is scoped to the owner.
    make_server(world.admin, name="Shared")
    assert AssistantMCPServer.objects.filter(name="Shared").count() == 2


# --------------------------------------------------------------------------- #
# CRUD API
# --------------------------------------------------------------------------- #


def test_create_list_and_delete_a_server(world):
    c = client_for(world.member)

    res = c.post(URL, {"name": "Tools", "url": "https://tools.example.com/mcp"}, format="json")
    assert res.status_code == 201, res.data
    server_id = res.data["id"]
    assert res.data["tool_prefix"] == "tools"
    assert res.data["has_auth_header"] is False

    res = c.get(URL)
    assert res.status_code == 200
    assert [s["id"] for s in res.data] == [server_id]

    assert c.delete(f"{URL}{server_id}/").status_code == 204
    assert c.get(URL).data == []


def test_auth_header_is_stored_encrypted_and_never_returned(world):
    c = client_for(world.member)
    res = c.post(
        URL,
        {"name": "Secured", "url": "https://tools.example.com/mcp", "auth_header": "Bearer s3cret"},
        format="json",
    )
    assert res.status_code == 201
    # Presence is reported; the value never is.
    assert res.data["has_auth_header"] is True
    assert "auth_header" not in res.data
    assert "s3cret" not in str(res.data)

    server = AssistantMCPServer.objects.get(id=res.data["id"])
    assert bytes(server.auth_header_encrypted) != b"Bearer s3cret"
    assert crypto.decrypt(server.auth_header_encrypted) == "Bearer s3cret"


def test_patch_without_auth_header_keeps_the_stored_credential(world):
    # Renaming a server must not silently drop its credential.
    c = client_for(world.member)
    created = c.post(
        URL,
        {"name": "Secured", "url": "https://tools.example.com/mcp", "auth_header": "Bearer keep"},
        format="json",
    ).data

    res = c.patch(f"{URL}{created['id']}/", {"name": "Renamed"}, format="json")
    assert res.status_code == 200
    assert res.data["has_auth_header"] is True
    server = AssistantMCPServer.objects.get(id=created["id"])
    assert crypto.decrypt(server.auth_header_encrypted) == "Bearer keep"


def test_empty_auth_header_clears_the_stored_credential(world):
    c = client_for(world.member)
    created = c.post(
        URL,
        {"name": "Secured", "url": "https://tools.example.com/mcp", "auth_header": "Bearer drop"},
        format="json",
    ).data

    res = c.patch(f"{URL}{created['id']}/", {"auth_header": ""}, format="json")
    assert res.status_code == 200
    assert res.data["has_auth_header"] is False
    assert AssistantMCPServer.objects.get(id=created["id"]).auth_header_encrypted is None


def test_toggling_is_enabled_round_trips(world):
    c = client_for(world.member)
    created = c.post(URL, {"name": "Tools", "url": "https://t.example.com/mcp"}, format="json").data
    res = c.patch(f"{URL}{created['id']}/", {"is_enabled": False}, format="json")
    assert res.status_code == 200
    assert res.data["is_enabled"] is False


@pytest.mark.parametrize(
    "url",
    [
        "ftp://tools.example.com/mcp",
        "javascript:alert(1)",
        "https://user:pw@tools.example.com/mcp",
        "",
    ],
)
def test_invalid_urls_are_rejected(world, url):
    c = client_for(world.member)
    res = c.post(URL, {"name": "Bad", "url": url}, format="json")
    assert res.status_code == 400


def test_duplicate_name_is_a_clean_400_not_a_500(world):
    c = client_for(world.member)
    c.post(URL, {"name": "Tools", "url": "https://a.example.com/mcp"}, format="json")
    res = c.post(URL, {"name": "Tools", "url": "https://b.example.com/mcp"}, format="json")
    assert res.status_code == 400
    assert res.data["error"] == "duplicate_name"


def test_rename_onto_an_existing_name_is_rejected(world):
    c = client_for(world.member)
    c.post(URL, {"name": "First", "url": "https://a.example.com/mcp"}, format="json")
    second = c.post(URL, {"name": "Second", "url": "https://b.example.com/mcp"}, format="json").data
    res = c.patch(f"{URL}{second['id']}/", {"name": "First"}, format="json")
    assert res.status_code == 400
    assert res.data["error"] == "duplicate_name"


def test_a_user_cannot_see_or_touch_another_users_server(world):
    other = make_server(world.admin, name="Private")
    c = client_for(world.member)

    assert c.get(URL).data == []
    assert c.patch(f"{URL}{other.id}/", {"name": "Hijacked"}, format="json").status_code == 404
    assert c.delete(f"{URL}{other.id}/").status_code == 404
    # Still intact and unrenamed.
    other.refresh_from_db()
    assert other.name == "Private"


def test_blocked_url_is_rejected_when_ssrf_blocking_is_on(world, settings):
    settings.ASSISTANT_BLOCK_PRIVATE_URLS = True
    c = client_for(world.member)
    res = c.post(URL, {"name": "Internal", "url": "http://127.0.0.1:9000/mcp"}, format="json")
    assert res.status_code == 400
    assert res.data["error"] == "url_blocked"


def test_anonymous_access_is_refused(world):
    assert APIClient().get(URL).status_code in (401, 403)


# --------------------------------------------------------------------------- #
# Toolset construction
# --------------------------------------------------------------------------- #


def test_only_enabled_servers_become_toolsets(world):
    make_server(world.member, name="On", is_enabled=True)
    make_server(world.member, name="Off", is_enabled=False)

    toolsets, skipped = mcp_runtime.build_toolsets(world.member)
    assert len(toolsets) == 1
    assert skipped == []


def test_each_server_gets_its_own_tool_prefix(world):
    # Distinct prefixes are what stop two servers (or a server and the
    # built-in tools) from exposing colliding tool names to the model.
    make_server(world.member, name="Alpha", url="https://a.example.com/mcp")
    make_server(world.member, name="Beta", url="https://b.example.com/mcp")

    toolsets, _ = mcp_runtime.build_toolsets(world.member)
    prefixes = sorted(t.prefix for t in toolsets)
    assert prefixes == ["alpha", "beta"]


def test_auth_header_is_decrypted_when_building_the_toolset(world, monkeypatch):
    server = make_server(world.member, name="Secured")
    server.auth_header_encrypted = crypto.encrypt("Bearer tok")
    server.save()

    seen: dict[str, object] = {}

    def capture(**kwargs):
        seen.update(kwargs)
        return object()

    monkeypatch.setattr(mcp_runtime, "build_toolset", capture)

    _, skipped = mcp_runtime.build_toolsets(world.member)
    assert skipped == []
    assert seen["auth_header"] == "Bearer tok"
    assert seen["url"] == "https://tools.example.com/mcp"


def test_a_server_without_an_auth_header_sends_none(world, monkeypatch):
    make_server(world.member, name="Open")
    seen: dict[str, object] = {}
    monkeypatch.setattr(
        mcp_runtime, "build_toolset", lambda **kw: (seen.update(kw), object())[1]
    )

    mcp_runtime.build_toolsets(world.member)
    assert seen["auth_header"] is None


def test_a_blocked_url_is_skipped_without_taking_down_the_others(world, settings, monkeypatch):
    # The failure mode this guards: one bad row silently costing the user every
    # tool, or worse, failing the whole turn.
    settings.ASSISTANT_BLOCK_PRIVATE_URLS = True
    make_server(world.member, name="Good", url="https://good.example.com/mcp")
    make_server(world.member, name="Internal", url="http://127.0.0.1:9000/mcp")

    # Pin DNS: the guard resolves hostnames, and this test must not depend on
    # what the CI network resolves (good.example.com has no real A record, and
    # an unresolvable host is treated as blocked).
    from pi_dash.assistant import ssrf

    def fake_getaddrinfo(host, _port):
        # NB: the RFC 5737 documentation ranges (203.0.113.0/24 etc.) count as
        # *private* to ipaddress.is_private, so a genuinely public unicast
        # address is required here.
        ip = "127.0.0.1" if host == "127.0.0.1" else "8.8.8.8"
        return [(None, None, None, None, (ip, 0))]

    monkeypatch.setattr(ssrf.socket, "getaddrinfo", fake_getaddrinfo)

    toolsets, skipped = mcp_runtime.build_toolsets(world.member)
    assert len(toolsets) == 1
    assert [(s.name, s.reason) for s in skipped] == [("Internal", "url_blocked")]


def test_an_undecryptable_auth_header_skips_only_that_server(world, monkeypatch):
    make_server(world.member, name="Good", url="https://good.example.com/mcp")
    broken = make_server(world.member, name="Broken", url="https://broken.example.com/mcp")
    broken.auth_header_encrypted = b"not-a-valid-ciphertext"
    broken.save()

    def boom(_token):
        raise ValueError("cannot decrypt")

    monkeypatch.setattr(mcp_runtime.crypto, "decrypt", boom)

    toolsets, skipped = mcp_runtime.build_toolsets(world.member)
    names = [s.name for s in skipped]
    # The row with no header still builds; only the broken one is skipped.
    assert "Broken" in names
    assert len(toolsets) == 1


def test_no_servers_is_an_empty_result_not_an_error(world):
    toolsets, skipped = mcp_runtime.build_toolsets(world.member)
    assert toolsets == []
    assert skipped == []


def test_ce_seam_returns_the_users_servers(world):
    from pi_dash.ee.assistant.model_provider import resolve_toolsets_for_user

    make_server(world.member, name="Tools")
    toolsets, skipped = resolve_toolsets_for_user(world.member)
    assert len(toolsets) == 1
    assert skipped == []


def test_servers_are_scoped_to_their_owner_when_building(world):
    make_server(world.admin, name="AdminTools")
    toolsets, _ = mcp_runtime.build_toolsets(world.member)
    assert toolsets == []


# --------------------------------------------------------------------------- #
# Resilience: a dead server must not cost the user their assistant
# --------------------------------------------------------------------------- #


def test_an_unreachable_server_does_not_fail_the_run():
    """The failure this guards is the whole point of the wrapper.

    Building a toolset does no I/O — the connection opens when the agent
    *enters* it. Unwrapped, a server that is down raises out of ``Agent.run``
    and takes the turn with it, so one broken tool server costs the user their
    assistant entirely.

    Driven via ``asyncio.run`` so the test needs no async plugin — the CI
    runner installs only ``requirements/test.txt``.
    """
    import asyncio

    from pydantic_ai import Agent

    # Port 9 (discard) refuses fast and deterministically.
    toolset = mcp_runtime.build_toolset(
        url="http://127.0.0.1:9/mcp", timeout=1, read_timeout=1, server_name="dead"
    )
    result = asyncio.run(Agent().run("hi", model="test", toolsets=[toolset]))

    assert result is not None  # the turn completed
    assert toolset.failure is not None  # ...and the failure was recorded
    assert toolset.server_name == "dead"


def test_a_dead_server_reports_no_tools_rather_than_raising():
    import asyncio

    toolset = mcp_runtime.build_toolset(
        url="http://127.0.0.1:9/mcp", timeout=1, read_timeout=1, server_name="dead"
    )

    async def scenario():
        async with toolset:
            return await toolset.get_tools(None)

    assert asyncio.run(scenario()) == {}


def test_the_wrapper_is_transparent_over_the_real_toolset():
    # It must add resilience without hiding what it wraps: the prefix and the
    # underlying MCP toolset stay reachable.
    from pydantic_ai.mcp import MCPToolset

    toolset = mcp_runtime.build_toolset(
        url="https://ok.example.com/mcp", tool_prefix="ok", server_name="Ok"
    )
    assert toolset.failure is None
    assert toolset.server_name == "Ok"
    assert toolset.wrapped.prefix == "ok"
    assert isinstance(toolset.wrapped.wrapped, MCPToolset)


def test_a_server_that_dies_mid_call_does_not_take_the_turn_with_it():
    """The fail-open has to cover ``call_tool``, not just connect and list.

    Connecting and listing succeed at turn start; the server can still time out
    or drop on the second tool call. pydantic-ai's tool manager converts only
    its own control-flow exceptions, so anything else raises straight out of
    ``Agent.run`` — the exact hard failure this wrapper exists to prevent.
    """
    import asyncio

    class _Boom:
        async def call_tool(self, name, tool_args, ctx, tool):
            raise ConnectionResetError("server went away")

        async def __aenter__(self):
            return self

        async def __aexit__(self, *args):
            return None

    toolset = mcp_runtime.ResilientToolset(_Boom(), server_name="flaky")
    result = asyncio.run(toolset.call_tool("search", {}, None, None))

    assert "unavailable" in result  # the model is told, as a tool result
    assert toolset.failure == "ConnectionResetError"  # ...and the user can be too


def test_a_tools_own_retry_is_not_mistaken_for_an_outage():
    # ModelRetry is the tool's considered answer, not a dead server; swallowing
    # it would turn a recoverable retry into a silent capability loss.
    import asyncio

    from pydantic_ai.exceptions import ModelRetry

    class _Retry:
        async def call_tool(self, name, tool_args, ctx, tool):
            raise ModelRetry("try narrower arguments")

    toolset = mcp_runtime.ResilientToolset(_Retry(), server_name="picky")

    with pytest.raises(ModelRetry):
        asyncio.run(toolset.call_tool("search", {}, None, None))
    assert toolset.failure is None


def test_the_wrapper_survives_being_rebuilt_by_pydantic_ai():
    """``dataclasses.replace`` must not blow up on this wrapper.

    pydantic-ai rebuilds wrappers that way in ``for_run``, ``for_run_step`` and
    ``visit_and_replace``. A required constructor argument made every rebuild a
    TypeError — dormant only because ``MCPToolset.for_run`` returns ``self``,
    and it would fire exactly where the wrapper is meant to prevent a hard
    failure.
    """
    import dataclasses

    toolset = mcp_runtime.build_toolset(url="https://ok.example.com/mcp", tool_prefix="ok", server_name="Ok")
    rebuilt = dataclasses.replace(toolset, wrapped=toolset.wrapped)

    assert isinstance(rebuilt, mcp_runtime.ResilientToolset)
    assert rebuilt.server_name == "Ok"  # configuration carries across
    assert rebuilt.prefix == "ok"


def test_the_read_timeout_is_not_tighter_than_pydantic_ais_own():
    # It is documented as generous; a value below the library default is the
    # opposite, and silently fails every long-running tool call.
    assert mcp_runtime.DEFAULT_READ_TIMEOUT_S >= 300.0
    assert mcp_runtime.DEFAULT_READ_TIMEOUT_S > mcp_runtime.DEFAULT_TIMEOUT_S


# --------------------------------------------------------------------------- #
# Tool-prefix collisions
# --------------------------------------------------------------------------- #


def test_names_that_slugify_alike_get_distinct_prefixes(world):
    # Names are unique per user, but slugification is lossy: all three of these
    # reduce to "my_tools". Colliding prefixes silently shadow one server's
    # tools with another's, which the model has no way to detect.
    for name in ("My Tools", "my-tools", "my_tools"):
        make_server(world.member, name=name, url=f"https://{name.replace(' ', '')}.example.com/mcp")

    toolsets, skipped = mcp_runtime.build_toolsets(world.member)
    prefixes = [t.wrapped.prefix for t in toolsets]

    assert skipped == []
    assert len(prefixes) == 3
    assert len(set(prefixes)) == 3, f"colliding prefixes: {prefixes}"
    assert prefixes[0] == "my_tools"  # the first keeps the natural prefix


def test_prefix_assignment_is_stable_for_unchanged_servers(world):
    make_server(world.member, name="Alpha", url="https://a.example.com/mcp")
    make_server(world.member, name="Beta", url="https://b.example.com/mcp")

    first = [t.wrapped.prefix for t in mcp_runtime.build_toolsets(world.member)[0]]
    second = [t.wrapped.prefix for t in mcp_runtime.build_toolsets(world.member)[0]]
    assert first == second


def test_the_list_view_reports_the_prefix_a_run_actually_assigns(world):
    # The row's own slug is not what the tools get once two servers collide.
    # Showing it would tell both servers they share a prefix when they do not.
    for name in ("My Tools", "my-tools"):
        make_server(world.member, name=name, url=f"https://{name.replace(' ', '')}.example.com/mcp")

    body = client_for(world.member).get(URL).json()
    effective = {row["name"]: row["effective_tool_prefix"] for row in body}

    assert {row["tool_prefix"] for row in body} == {"my_tools"}  # both slugify alike...
    assert len(set(effective.values())) == 2, effective  # ...but get distinct prefixes
    assert effective["My Tools"] == "my_tools"


def test_a_disabled_server_claims_no_prefix(world):
    # It contributes no toolset, so promising it one would be a lie — and would
    # push an enabled server onto a counter suffix it never actually gets.
    make_server(world.member, name="Off", url="https://off.example.com/mcp", is_enabled=False)

    row = client_for(world.member).get(URL).json()[0]
    assert row["effective_tool_prefix"] is None


# --------------------------------------------------------------------------- #
# Server count is bounded
# --------------------------------------------------------------------------- #


def test_creating_past_the_cap_is_refused(world, settings):
    # Toolsets are entered sequentially at the start of every turn, so an
    # unbounded list is dead time the user pays on every single message.
    settings.ASSISTANT_MCP_MAX_SERVERS = 2
    for i in range(2):
        make_server(world.member, name=f"S{i}", url=f"https://s{i}.example.com/mcp")

    res = client_for(world.member).post(
        URL, {"name": "One too many", "url": "https://extra.example.com/mcp"}, format="json"
    )

    assert res.status_code == 400
    assert res.json()["error"] == "too_many_servers"
    assert AssistantMCPServer.objects.filter(user=world.member).count() == 2


def test_rows_over_the_cap_are_skipped_rather_than_silently_run(world, settings):
    # Rows can predate the cap or outlive a lowered one. Dropping them silently
    # would leave the user wondering why their newest server never runs.
    for i in range(3):
        make_server(world.member, name=f"S{i}", url=f"https://s{i}.example.com/mcp")
    settings.ASSISTANT_MCP_MAX_SERVERS = 2

    toolsets, skipped = mcp_runtime.build_toolsets(world.member)

    assert len(toolsets) == 2
    assert [s.name for s in skipped] == ["S2"]
    assert skipped[0].reason == "too_many_servers"


# --------------------------------------------------------------------------- #
# Review fixes: race-safe create, error classification, resolver blow-up
# --------------------------------------------------------------------------- #


def test_a_create_race_on_the_name_returns_400_not_500(world, monkeypatch):
    # The pre-check can pass for two concurrent creates; the unique constraint
    # then rejects the loser. That must surface as the same 400 the pre-check
    # would have produced, not a 500.
    from django.db import IntegrityError

    def boom(self, *args, **kwargs):
        raise IntegrityError("duplicate key value violates unique constraint")

    monkeypatch.setattr(AssistantMCPServer, "save", boom)
    res = client_for(world.member).post(
        URL, {"name": "Raced", "url": "https://raced.example.com/mcp"}, format="json"
    )
    assert res.status_code == 400
    assert res.data["error"] == "duplicate_name"


def test_out_of_credit_is_classified_by_status_code_not_substring():
    from pi_dash.assistant.tasks import _classify_error

    class Provider402(Exception):
        status_code = 402

    code, _ = _classify_error(Provider402("Payment Required"))
    assert code == "provider_out_of_credit"

    # "402" appearing inside a request id must not trip the credit branch.
    code, _ = _classify_error(Exception("connection reset, request id req_a402bfe9"))
    assert code != "provider_out_of_credit"

    code, _ = _classify_error(Exception("Error code: insufficient_credits"))
    assert code == "provider_out_of_credit"


def test_a_resolver_blow_up_still_tells_the_user(monkeypatch):
    # Per-server failures emit tool_servers_skipped; the total-failure branch
    # must not be the one silent case.
    from types import SimpleNamespace

    from pi_dash.assistant import tasks as assistant_tasks

    emitted = []
    monkeypatch.setattr(
        assistant_tasks, "_emit_skipped", lambda ctx, servers: emitted.append(servers)
    )

    def exploding_resolver(user):
        raise RuntimeError("resolver dead")

    ctx = SimpleNamespace(user=None, thread=None, turn=None)
    toolsets, skipped = assistant_tasks._resolve_toolsets(ctx, exploding_resolver)
    assert toolsets == []
    assert skipped == []
    assert emitted == [[{"name": "all tool servers", "reason": "toolsets_unavailable"}]]
