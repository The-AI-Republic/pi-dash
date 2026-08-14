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


def test_a_blocked_url_is_skipped_without_taking_down_the_others(world, settings):
    # The failure mode this guards: one bad row silently costing the user every
    # tool, or worse, failing the whole turn.
    settings.ASSISTANT_BLOCK_PRIVATE_URLS = True
    make_server(world.member, name="Good", url="https://good.example.com/mcp")
    make_server(world.member, name="Internal", url="http://127.0.0.1:9000/mcp")

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
