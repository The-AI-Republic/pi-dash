# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""BYOK LLM config and title generation.

Covers ``users/me/ai-assistant/config/`` (lifecycle, key secrecy, serializer
validation, the SSRF save-time guard) and ``generate-title/`` (the
deterministic gates: config missing, blank description, guest denial).
The title-generation wire shape is pinned against a recording proxy because
the provider URL must stay on a public DeepSeek host for the SSRF guard.
The ``/test/`` probe is covered through its missing-config shape.
"""

import json
import os
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from .conftest import PUBLIC_BASE, threads_url

CONFIG_URL = "/api/users/me/ai-assistant/config/"
TEST_URL = "/api/users/me/ai-assistant/config/test/"
DEEPSEEK_BASE = "http://api.deepseek.com/v1"
PROVIDER_PROXY = "http://127.0.0.1:18097"


def _unset_shape(body):
    assert set(body.keys()) >= {
        "provider_kind",
        "base_url",
        "model_name",
        "has_api_key",
        "last_verified_at",
    }
    assert body["has_api_key"] is False
    assert body["last_verified_at"] is None


def test_llm_config_lifecycle(world, member):
    assert member.get(CONFIG_URL).status_code == 200
    _unset_shape(member.get(CONFIG_URL).json())

    res = member.put(
        CONFIG_URL,
        json={
            "provider_kind": "openai_compatible",
            "base_url": PUBLIC_BASE,
            "model_name": "m",
            "api_key": "test-key-123",
        },
    )
    assert res.status_code == 200
    body = res.json()
    assert body["has_api_key"] is True
    assert body["base_url"] == PUBLIC_BASE
    assert body["model_name"] == "m"
    assert "api_key" not in body

    # Updating the model without resending the key keeps the stored key.
    res = member.put(CONFIG_URL, json={"model_name": "m2"})
    assert res.status_code == 200
    assert res.json()["has_api_key"] is True
    assert res.json()["model_name"] == "m2"

    assert member.delete(CONFIG_URL).status_code == 204
    assert member.get(CONFIG_URL).json()["has_api_key"] is False


def test_llm_config_is_per_user(world, member, admin):
    admin.put(
        CONFIG_URL,
        json={
            "provider_kind": "openai_compatible",
            "base_url": PUBLIC_BASE,
            "model_name": "m",
            "api_key": "test-key-123",
        },
    )
    assert member.get(CONFIG_URL).json()["has_api_key"] is False


def test_llm_config_validation(world, member):
    # Non-http(s) scheme.
    res = member.put(
        CONFIG_URL,
        json={"provider_kind": "openai_compatible", "base_url": "ftp://x/v1",
              "model_name": "m", "api_key": "test-key-123"},
    )
    assert res.status_code == 400

    # Credentials embedded in the URL.
    res = member.put(
        CONFIG_URL,
        json={"provider_kind": "openai_compatible", "base_url": "https://u:p@8.8.8.8/v1",
              "model_name": "m", "api_key": "test-key-123"},
    )
    assert res.status_code == 400

    # OpenAI-compatible providers require a base URL and model name.
    res = member.put(
        CONFIG_URL,
        json={"provider_kind": "openai_compatible", "base_url": "",
              "model_name": "m", "api_key": "test-key-123"},
    )
    assert res.status_code == 400

    res = member.put(
        CONFIG_URL,
        json={"provider_kind": "openai_compatible", "base_url": PUBLIC_BASE,
              "model_name": "", "api_key": "test-key-123"},
    )
    assert res.status_code == 400

    # Keys that are implausibly short are rejected.
    res = member.put(
        CONFIG_URL,
        json={"provider_kind": "openai_compatible", "base_url": PUBLIC_BASE,
              "model_name": "m", "api_key": "short"},
    )
    assert res.status_code == 400


def test_llm_config_rejects_blocked_url_at_save(world, member):
    res = member.put(
        CONFIG_URL,
        json={"provider_kind": "openai_compatible",
              "base_url": "http://169.254.169.254/v1",
              "model_name": "m", "api_key": "test-key-123"},
    )
    assert res.status_code == 400
    assert res.json()["error"] == "base_url_blocked"


def test_llm_test_missing_config(world, member):
    res = member.post(TEST_URL)
    assert res.status_code == 200
    body = res.json()
    assert body["ok"] is False
    assert body["error_code"] == "llm_config_missing"


def test_llm_test_requires_auth(world, anon):
    assert anon.post(TEST_URL).status_code == 401


def test_generate_title_requires_llm_config(world, member):
    res = member.post(
        f"{threads_url(world)}/generate-title/",
        json={"description": "Ship the new dashboard export."},
    )
    assert res.status_code == 422
    assert res.json()["error"] == "llm_config_missing"


def test_generate_title_requires_description(world, member):
    member.put(
        CONFIG_URL,
        json={"provider_kind": "openai_compatible", "base_url": PUBLIC_BASE,
              "model_name": "m", "api_key": "test-key-123"},
    )
    res = member.post(f"{threads_url(world)}/generate-title/", json={"description": "   "})
    assert res.status_code == 400
    assert res.json()["error"] == "description_required"


def test_generate_title_blocked_for_guest(world, guest):
    res = guest.post(
        f"{threads_url(world)}/generate-title/",
        json={"description": "Ship the new dashboard export."},
    )
    assert res.status_code == 403


class _RecordingDeepSeekProxy:
    """HTTP proxy that records the backend's title request and replies like OpenAI."""

    def __init__(self):
        self.requests = []
        self._server = None
        self._thread = None

    def start(self):
        stub = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.0"

            def log_message(self, *args):
                pass

            def do_POST(self):
                length = int(self.headers.get("Content-Length", 0))
                raw = self.rfile.read(length) if length else b""
                try:
                    body = json.loads(raw or b"{}")
                except ValueError:
                    body = {}
                stub.requests.append({"path": self.path, "body": body})
                response = json.dumps(
                    {
                        "id": "stub",
                        "object": "chat.completion",
                        "created": 0,
                        "model": "stub-model",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "DeepSeek stub title.",
                                },
                                "finish_reason": "stop",
                            }
                        ],
                    }
                ).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(response)))
                self.end_headers()
                self.wfile.write(response)

        host, port = PROVIDER_PROXY.removeprefix("http://").split(":", 1)
        self._server = ThreadingHTTPServer((host, int(port)), Handler)
        self._server.daemon_threads = True
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()

    def stop(self):
        if self._server is not None:
            self._server.shutdown()
            self._server.server_close()
            self._server = None


def test_generate_title_deepseek_body_flattens_extra_body(world, member):
    """Django's OpenAI SDK turns ``extra_body`` into top-level ``thinking``.

    The backend under test runs with the SSRF guard on, so the provider URL is
    the public DeepSeek host. The contract runner sets ``HTTP_PROXY`` to this
    recording proxy for both backends; ``NO_PROXY`` keeps the pytest client's
    calls to ``BASE_URL`` direct.
    """
    proxy_env = os.environ.get("HTTP_PROXY") or os.environ.get("http_proxy")
    assert proxy_env == PROVIDER_PROXY, (
        "assistant contract runs must point the backend HTTP_PROXY at "
        f"{PROVIDER_PROXY} so the DeepSeek wire body can be captured"
    )

    proxy = _RecordingDeepSeekProxy()
    proxy.start()
    try:
        res = member.put(
            CONFIG_URL,
            json={
                "provider_kind": "openai_compatible",
                "base_url": DEEPSEEK_BASE,
                "model_name": "stub-model",
                "api_key": "test-key-123",
            },
        )
        assert res.status_code == 200, res.text

        res = member.post(
            f"{threads_url(world)}/generate-title/",
            json={"description": "Ship the new dashboard export."},
        )
        assert res.status_code == 200, res.text
        assert res.json() == {"title": "DeepSeek stub title"}

        assert len(proxy.requests) == 1
        request = proxy.requests[0]
        assert request["path"] == f"{DEEPSEEK_BASE}/chat/completions"
        body = request["body"]
        assert body.get("thinking") == {"type": "disabled"}, body
        assert "extra_body" not in body, body
    finally:
        proxy.stop()
