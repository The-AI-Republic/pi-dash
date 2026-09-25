# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""MCP tool servers: per-user CRUD contract.

Covers ``users/me/ai-assistant/mcp-servers/`` — create/list shape (including
``tool_prefix`` and ``effective_tool_prefix``), the write-only auth header
(presence reported, value never returned), rename-without-credential-loss,
explicit clearing, enable toggling, URL validation, duplicate names, the
per-user scope (another user's server is invisible: 404), the server cap,
and the SSRF guard. This is also the "installed runners" wire surface for
this domain: each row becomes one namespaced toolset at run time.
"""

import uuid

MCP_URL = "/api/users/me/ai-assistant/mcp-servers/"
SERVER_URL = "https://8.8.8.8/mcp"


def _create(client, name="Tools", **overrides):
    payload = {"name": name, "url": SERVER_URL}
    payload.update(overrides)
    return client.post(MCP_URL, json=payload)


def test_create_list_and_delete_server(world, member):
    res = _create(member)
    assert res.status_code == 201, res.text
    body = res.json()
    assert body["tool_prefix"] == "mcp_tools"
    assert body["has_auth_header"] is False
    assert "auth_header" not in body

    listed = member.get(MCP_URL)
    assert listed.status_code == 200
    assert [s["id"] for s in listed.json()] == [body["id"]]
    assert listed.json()[0]["effective_tool_prefix"] == "mcp_tools"

    assert member.delete(f"{MCP_URL}{body['id']}/").status_code == 204
    assert member.get(MCP_URL).json() == []


def test_auth_header_is_write_only(world, member):
    res = _create(member, name="Secured", auth_header="Bearer s3cret-value")
    assert res.status_code == 201
    body = res.json()
    assert body["has_auth_header"] is True
    assert "auth_header" not in body
    assert "s3cret-value" not in res.text


def test_patch_without_auth_header_keeps_credential(world, member):
    created = _create(member, name="Secured", auth_header="Bearer s3cret-value").json()
    res = member.patch(f"{MCP_URL}{created['id']}/", json={"name": "Renamed"})
    assert res.status_code == 200
    assert res.json()["has_auth_header"] is True
    assert res.json()["name"] == "Renamed"


def test_empty_auth_header_clears_credential(world, member):
    created = _create(member, name="Secured", auth_header="Bearer s3cret-value").json()
    res = member.patch(f"{MCP_URL}{created['id']}/", json={"auth_header": ""})
    assert res.status_code == 200
    assert res.json()["has_auth_header"] is False


def test_toggle_is_enabled_round_trips(world, member):
    created = _create(member).json()
    res = member.patch(f"{MCP_URL}{created['id']}/", json={"is_enabled": False})
    assert res.status_code == 200
    assert res.json()["is_enabled"] is False


def test_invalid_urls_are_rejected(world, member):
    for url in (
        "ftp://tools.example.com/mcp",
        "javascript:alert(1)",
        "https://user:pw@8.8.8.8/mcp",
        "",
    ):
        res = _create(member, name=f"Bad-{uuid.uuid4().hex[:6]}", url=url)
        assert res.status_code == 400, url


def test_duplicate_name_is_a_clean_400(world, member):
    assert _create(member, name="Tools", url="https://8.8.8.8/a").status_code == 201
    res = _create(member, name="Tools", url="https://8.8.8.9/b")
    assert res.status_code == 400
    assert res.json()["error"] == "duplicate_name"


def test_rename_onto_existing_name_is_rejected(world, member):
    _create(member, name="First")
    second = _create(member, name="Second").json()
    res = member.patch(f"{MCP_URL}{second['id']}/", json={"name": "First"})
    assert res.status_code == 400
    assert res.json()["error"] == "duplicate_name"


def test_user_cannot_see_or_touch_another_users_server(world, member, admin):
    other = _create(admin, name="Private").json()

    assert member.get(MCP_URL).json() == []
    assert (
        member.patch(f"{MCP_URL}{other['id']}/", json={"name": "Hijacked"}).status_code
        == 404
    )
    assert member.delete(f"{MCP_URL}{other['id']}/").status_code == 404
    assert [s["name"] for s in admin.get(MCP_URL).json()] == ["Private"]


def test_server_cap_is_enforced(world, member):
    for i in range(10):
        res = _create(member, name=f"Srv-{i}")
        assert res.status_code == 201, res.text
    res = _create(member, name="One-too-many")
    assert res.status_code == 400
    assert res.json()["error"] == "too_many_servers"


def test_blocked_url_is_rejected(world, member):
    res = _create(member, name="Internal", url="http://127.0.0.1:9000/mcp")
    assert res.status_code == 400
    assert res.json()["error"] == "url_blocked"


def test_anonymous_access_is_refused(world, anon):
    assert anon.get(MCP_URL).status_code == 401
