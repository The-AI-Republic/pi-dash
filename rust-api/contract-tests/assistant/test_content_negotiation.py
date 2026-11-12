# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""DRF body negotiation on the thread/message/MCP write paths (PIDASHCONV-1240).

These views read ``request.data``, so they inherit DRF's default parsers
(JSON, urlencoded form, multipart) selected by Content-Type, and a 415
``UnsupportedMediaType`` for anything else. The Rust handlers used to
JSON-decode every body and ignore Content-Type, so form posts failed and
foreign-typed bodies were served.

Covers the five write handlers named by the issue: thread create/patch,
message POST, MCP create/patch — form bodies parse (urlencoded and
multipart text), file parts in scalar positions keep Django's error
shapes, and ``text/plain`` bodies are refused with the exact 415 detail.
Bodies with no Content-Type at all are refused the same way, echoing the
empty media type (PIDASHCONV-1414), on one endpoint per parse_body
consumer family.
"""

from .conftest import PUBLIC_BASE, threads_url

MCP_URL = "/api/users/me/ai-assistant/mcp-servers/"
SERVER_URL = "https://8.8.8.8/mcp"
LLM_CONFIG_URL = "/api/users/me/ai-assistant/config/"
STT_CONFIG_URL = "/api/users/me/ai-assistant/stt-config/"
FORM = {"Content-Type": "application/x-www-form-urlencoded"}
PLAIN = {"Content-Type": "text/plain"}
UNSUPPORTED = {"detail": 'Unsupported media type "text/plain" in request.'}
MISSING_CT = {"detail": 'Unsupported media type "" in request.'}
GENERIC_500 = {"error": "Something went wrong please try again later"}


def _without_content_type(client, method, url, body: bytes):
    request = client.build_request(method, url, content=body)
    assert "content-type" not in request.headers
    return client.send(request)


def _configure_llm(client):
    res = client.put(
        "/api/users/me/ai-assistant/config/",
        json={
            "provider_kind": "openai_compatible",
            "base_url": PUBLIC_BASE,
            "model_name": "m",
            "api_key": "test-key-123",
        },
    )
    assert res.status_code == 200, res.text


def _new_thread(client, base, title=""):
    res = client.post(f"{base}/threads/", json={"title": title})
    assert res.status_code == 201, res.text
    return res.json()["id"]


def test_form_encoded_thread_create_parses(world, member):
    res = member.post(
        f"{threads_url(world)}/threads/", content="title=x", headers=FORM
    )
    assert res.status_code == 201, res.text
    assert res.json()["title"] == "x"


def test_form_encoded_thread_patch_parses(world, member):
    base = threads_url(world)
    tid = _new_thread(member, base)
    # `bool("false")` is True: form values are strings, and any non-empty
    # string is truthy in the PATCH `is_archived` coercion.
    res = member.patch(
        f"{base}/threads/{tid}/",
        content="title=y&is_archived=false",
        headers=FORM,
    )
    assert res.status_code == 200, res.text
    body = res.json()
    assert body["title"] == "y"
    assert body["is_archived"] is True


def test_form_encoded_mcp_create_parses(world, member):
    res = member.post(
        MCP_URL,
        content=f"name=a&url={SERVER_URL}",
        headers=FORM,
    )
    assert res.status_code == 201, res.text
    body = res.json()
    assert body["name"] == "a"
    assert body["url"] == SERVER_URL
    # HTML forms cannot express an unchecked checkbox: a missing
    # `is_enabled` arrives as False (`default_empty_html`), while the
    # JSON path omits it and the view defaults to True.
    assert body["is_enabled"] is False


def test_json_mcp_create_defaults_is_enabled_true(world, member):
    res = member.post(MCP_URL, json={"name": "j", "url": SERVER_URL})
    assert res.status_code == 201, res.text
    assert res.json()["is_enabled"] is True


def test_form_encoded_mcp_patch_parses(world, member):
    created = member.post(MCP_URL, json={"name": "mp", "url": SERVER_URL}).json()
    res = member.patch(
        f"{MCP_URL}{created['id']}/", content="name=renamed", headers=FORM
    )
    assert res.status_code == 200, res.text
    assert res.json()["name"] == "renamed"


def test_form_encoded_message_post_queues_turn(world, member):
    base = threads_url(world)
    _configure_llm(member)
    tid = _new_thread(member, base)
    res = member.post(
        f"{base}/threads/{tid}/messages/", content="content=hi", headers=FORM
    )
    assert res.status_code == 202, res.text
    assert res.json()["turn"]["status"] == "queued"
    assert res.json()["message"]["content"] == "hi"


def test_text_plain_body_is_unsupported_media_type(world, member):
    base = threads_url(world)
    _configure_llm(member)
    tid = _new_thread(member, base)

    res = member.post(
        f"{base}/threads/", content='{"title":"x"}', headers=PLAIN
    )
    assert res.status_code == 415, res.text
    assert res.json() == UNSUPPORTED

    res = member.post(
        f"{base}/threads/{tid}/messages/",
        content='{"content":"hi"}',
        headers=PLAIN,
    )
    assert res.status_code == 415, res.text
    assert res.json() == UNSUPPORTED

    res = member.post(
        MCP_URL, content='{"name":"a"}', headers=PLAIN
    )
    assert res.status_code == 415, res.text
    assert res.json() == UNSUPPORTED


def test_missing_content_type_is_unsupported_media_type(world, member):
    base = threads_url(world)
    tid = _new_thread(member, base)
    for method, url, body in (
        ("PUT", LLM_CONFIG_URL, b'{"model_name":"m"}'),
        ("PUT", STT_CONFIG_URL, b'{"model_name":"w"}'),
        ("POST", f"{base}/threads/", b'{"title":"x"}'),
        ("POST", f"{base}/threads/{tid}/messages/", b'{"content":"hi"}'),
        ("POST", MCP_URL, b'{"name":"a","url":"https://8.8.8.8/mcp"}'),
    ):
        res = _without_content_type(member, method, url, body)
        assert res.status_code == 415, (method, url, res.text)
        assert res.json() == MISSING_CT, (method, url, res.text)


def test_multipart_text_fields_parse_as_form(world, member):
    base = threads_url(world)
    res = member.post(
        f"{base}/threads/",
        data={"title": "t"},
        files={"attachment": ("a.txt", b"x")},
    )
    assert res.status_code == 201, res.text
    assert res.json()["title"] == "t"

    res = member.post(
        MCP_URL,
        data={"name": "mp1", "url": SERVER_URL},
        files={"attachment": ("a.txt", b"x")},
    )
    assert res.status_code == 201, res.text
    assert res.json()["name"] == "mp1"


def test_file_part_in_scalar_position_keeps_django_errors(world, member):
    base = threads_url(world)
    # An uploaded file where `.strip()`/`[:255]` runs: AttributeError /
    # TypeError, which the base view maps to the generic 500.
    res = member.post(
        f"{base}/threads/", files={"title": ("n.txt", b"xx")}
    )
    assert res.status_code == 500, res.text
    assert res.json() == GENERIC_500

    # The MCP serializer rejects the UploadedFile per field.
    res = member.post(
        MCP_URL,
        data={"url": SERVER_URL},
        files={"name": ("n.txt", b"xx")},
    )
    assert res.status_code == 400, res.text
    assert res.json() == {"name": ["Not a valid string."]}
