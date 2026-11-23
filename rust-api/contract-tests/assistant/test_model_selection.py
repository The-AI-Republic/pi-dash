# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-surface model choice (NEWPIDASH-392, OSS-1, DIFF-64).

Covers ``GET /api/users/me/ai-assistant/model-selection/``,
``PUT .../model-selection/{surface}/`` and
``DELETE .../model-selection/{surface}/``: auth, unknown surfaces, the
saved/effective/fallback_reason shapes, per-surface isolation, the
agent-surface refusal, and turn surface stamping.

The endpoints are new-system-only: Django has no such routes, so the
Django arm pins 404 while the Rust arm pins the §6.1 behaviour
(``expect("DIFF-64", ...)`` throughout).
"""

from __future__ import annotations

import os
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from _harness.db import db_cursor
from _harness.diffs import expect, is_rust

from .conftest import threads_url

CONFIG_URL = "/api/users/me/ai-assistant/config/"
SELECTION_URL = "/api/users/me/ai-assistant/model-selection/"
DEEPSEEK_BASE = "http://api.deepseek.com/v1"
PROVIDER_PROXY = "http://127.0.0.1:18097"


def _item(surface):
    return f"{SELECTION_URL}{surface}/"


def _require_proxy_env():
    proxy_env = os.environ.get("HTTP_PROXY") or os.environ.get("http_proxy")
    assert proxy_env == PROVIDER_PROXY, (
        "assistant contract runs must point the backend HTTP_PROXY at "
        f"{PROVIDER_PROXY} so the model stub can answer"
    )


class _ModelStubProxy:
    """HTTP proxy answering provider probes per model ID in the body.

    The backend under test runs with the SSRF guard on, so the provider
    URL stays the public DeepSeek host; ``HTTP_PROXY`` points the backend
    at this stub (same shape as the title recorder in
    ``test_llm_config.py``). ``ok-*`` models probe ``verified``,
    ``gone-model`` probes ``failed`` (404 model-not-found),
    ``bad-model`` probes ``failed`` (401), anything else probes
    ``unverified`` (500).
    """

    RESPONSES = {
        "ok-web": (200, {"id": "stub", "object": "chat.completion", "created": 0, "model": "stub-model", "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}]}),
        "ok-desk": (200, {"id": "stub", "object": "chat.completion", "created": 0, "model": "stub-model", "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}]}),
        "ok-agent": (200, {"id": "stub", "object": "chat.completion", "created": 0, "model": "stub-model", "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}]}),
        "ok-a": (200, {"id": "stub", "object": "chat.completion", "created": 0, "model": "stub-model", "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}]}),
        "ok-b": (200, {"id": "stub", "object": "chat.completion", "created": 0, "model": "stub-model", "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}]}),
        "gone-model": (404, {"error": {"message": "model not found"}}),
        "bad-model": (401, {"error": {"message": "Incorrect API key"}}),
    }

    def __init__(self):
        import json

        stub = self
        self._json = json
        self._server = None
        self._thread = None

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.0"

            def log_message(self, *args):
                pass

            def do_POST(self):
                length = int(self.headers.get("Content-Length", 0))
                raw = self.rfile.read(length) if length else b""
                try:
                    body = stub._json.loads(raw or b"{}")
                except ValueError:
                    body = {}
                model = body.get("model", "") if isinstance(body, dict) else ""
                status, payload = stub.RESPONSES.get(model, (500, {"error": "boom"}))
                response = stub._json.dumps(payload).encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(response)))
                self.end_headers()
                self.wfile.write(response)

        host, port = PROVIDER_PROXY.removeprefix("http://").split(":", 1)
        self._server = ThreadingHTTPServer((host, int(port)), Handler)
        self._server.daemon_threads = True
        self._thread = threading.Thread(
            target=self._server.serve_forever, daemon=True
        )

    def start(self):
        self._thread.start()
        return self

    def stop(self):
        if self._server is not None:
            self._server.shutdown()
            self._server.server_close()
            self._server = None


def _configure_key(client, models):
    """Save a BYOK key holding ``models`` (all stub-verified).

    ``model_name`` rides along so the Django arm (which ignores the
    unknown ``models`` field) still saves: the Django half proves the
    old system ignores the new fields.
    """
    res = client.put(
        CONFIG_URL,
        json={
            "provider_kind": "openai_compatible",
            "base_url": DEEPSEEK_BASE,
            "model_name": models[0],
            "models": models,
            "api_key": "test-key-123",
        },
    )
    assert res.status_code == 200, res.text
    return res.json()


def test_model_selection_requires_auth(world, anon):
    # Collection GET is owned: 401 without a session on Rust.
    res = anon.get(SELECTION_URL)
    assert res.status_code == expect("DIFF-64", django=404, rust=401)
    # PUT/DELETE are owned: 401 without a session on Rust.
    res = anon.put(_item("web_assistant"), json={"model": "x"})
    assert res.status_code == expect("DIFF-64", django=404, rust=401), res.text
    res = anon.delete(_item("web_assistant"))
    assert res.status_code == expect("DIFF-64", django=404, rust=401), res.text
    # GET on an item path is not an endpoint: it proxies to Django,
    # which 404s on both backends.
    res = anon.get(_item("web_assistant"))
    assert res.status_code == 404


def test_model_selection_unknown_surface(world, member):
    res = member.put(_item("browser"), json={"model": "x"})
    assert res.status_code == expect("DIFF-64", django=404, rust=400), res.text
    if is_rust():
        assert res.json()["error"] == "unknown_surface"
    res = member.delete(_item("browser"))
    assert res.status_code == expect("DIFF-64", django=404, rust=400), res.text
    if is_rust():
        assert res.json()["error"] == "unknown_surface"


def test_model_selection_empty_shapes(world, member):
    """A fresh user: nothing saved, legacy lane empty."""
    if not is_rust():
        assert member.get(SELECTION_URL).status_code == 404
        return
    body = member.get(SELECTION_URL).json()
    assert set(body["surfaces"]) == {"web_assistant", "desktop_assistant", "agent"}
    for surface in ("web_assistant", "desktop_assistant"):
        entry = body["surfaces"][surface]
        assert entry == {"saved": None, "effective": None, "fallback_reason": "none_saved"}
    agent = body["surfaces"]["agent"]
    assert agent == {"saved": None, "effective": None, "fallback_reason": "lane_unavailable"}


def test_model_selection_roundtrip(world, member):
    """PUT validates and saves one surface; GET renders it; DELETE clears."""
    _require_proxy_env()
    stub = _ModelStubProxy().start()
    try:
        _configure_key(member, ["ok-web", "ok-desk"])
        if not is_rust():
            # Django has no such routes; the config PUT above already
            # proved the Django arm (unknown `models` field handling is
            # pinned in test_byok_models.py).
            assert member.get(SELECTION_URL).status_code == 404
            return

        # Save a web choice.
        res = member.put(_item("web_assistant"), json={"model": "ok-desk"})
        assert res.status_code == 200, res.text
        assert res.json() == {
            "saved": {"model": "ok-desk"},
            "effective": {"model": "ok-desk"},
            "fallback_reason": None,
        }

        # Desktop and agent are untouched; desktop falls back to the key's
        # first model with `none_saved`.
        body = member.get(SELECTION_URL).json()["surfaces"]
        assert body["web_assistant"]["saved"] == {"model": "ok-desk"}
        assert body["desktop_assistant"] == {
            "saved": None,
            "effective": {"model": "ok-web"},
            "fallback_reason": "none_saved",
        }
        assert body["agent"] == {
            "saved": None,
            "effective": None,
            "fallback_reason": "lane_unavailable",
        }

        # An unavailable model is rejected and changes nothing.
        res = member.put(_item("web_assistant"), json={"model": "nope"})
        assert res.status_code == 400
        assert res.json()["error"] == "model_not_available"
        assert (
            member.get(SELECTION_URL).json()["surfaces"]["web_assistant"]["saved"]
            == {"model": "ok-desk"}
        )

        # Clearing restores the default.
        assert member.delete(_item("web_assistant")).status_code == 204
        entry = member.get(SELECTION_URL).json()["surfaces"]["web_assistant"]
        assert entry["saved"] is None
        assert entry["effective"] == {"model": "ok-web"}
        assert entry["fallback_reason"] == "none_saved"
    finally:
        stub.stop()


def test_model_selection_per_surface_isolation(world, member):
    """Choices are stored per surface; writes never cross surfaces."""
    _require_proxy_env()
    stub = _ModelStubProxy().start()
    try:
        _configure_key(member, ["ok-web", "ok-desk"])
        if not is_rust():
            assert member.get(SELECTION_URL).status_code == 404
            return
        assert member.put(_item("web_assistant"), json={"model": "ok-desk"}).status_code == 200
        assert member.put(_item("desktop_assistant"), json={"model": "ok-web"}).status_code == 200
        body = member.get(SELECTION_URL).json()["surfaces"]
        assert body["web_assistant"]["saved"] == {"model": "ok-desk"}
        assert body["desktop_assistant"]["saved"] == {"model": "ok-web"}

        assert member.delete(_item("web_assistant")).status_code == 204
        body = member.get(SELECTION_URL).json()["surfaces"]
        assert body["web_assistant"]["saved"] is None
        assert body["desktop_assistant"]["saved"] == {"model": "ok-web"}
    finally:
        stub.stop()


def test_model_selection_agent_never_takes_byok(world, member):
    """The Pi Dash Agent stays OpenHub-only: BYOK is refused per surface."""
    _require_proxy_env()
    stub = _ModelStubProxy().start()
    try:
        _configure_key(member, ["ok-agent"])
        if not is_rust():
            assert member.get(SELECTION_URL).status_code == 404
            return
        res = member.put(_item("agent"), json={"model": "ok-agent"})
        assert res.status_code == 400
        assert res.json()["error"] == "not_allowed_for_surface"
        # A connection id names nothing on OSS.
        res = member.put(
            _item("web_assistant"),
            json={
                "model": "ok-agent",
                "connection_id": "6f1c2b3a-9d4e-4f5a-8b6c-7d8e9f0a1b2c",
            },
        )
        assert res.status_code == 400
        assert res.json()["error"] == "connection_not_found"
    finally:
        stub.stop()


def test_model_selection_removed_model_falls_back(world, member):
    """A saved ID that leaves the key's list resolves to the legacy
    model with `model_removed`."""
    _require_proxy_env()
    stub = _ModelStubProxy().start()
    try:
        _configure_key(member, ["ok-web", "ok-desk"])
        if not is_rust():
            assert member.get(SELECTION_URL).status_code == 404
            return
        assert member.put(_item("web_assistant"), json={"model": "ok-desk"}).status_code == 200
        # Shrink the key's list: no new IDs, so no probe runs.
        res = member.put(CONFIG_URL, json={"models": ["ok-web"]})
        assert res.status_code == 200, res.text
        entry = member.get(SELECTION_URL).json()["surfaces"]["web_assistant"]
        assert entry == {
            "saved": {"model": "ok-desk"},
            "effective": {"model": "ok-web"},
            "fallback_reason": "model_removed",
        }
    finally:
        stub.stop()


def test_turn_surface_stamping(world, member, desktop_member):
    """`post_message` stamps the request's surface on the turn; the
    client never sends it. Rust-only: Django has no `surface` column.
    """
    if not is_rust():
        return
    _require_proxy_env()
    stub = _ModelStubProxy().start()
    try:
        _configure_key(member, ["ok-web"])
        base = threads_url(world)
        for client in (member, desktop_member):
            res = client.post(f"{base}/threads/", json={"title": "t"})
            assert res.status_code == 201, res.text
            tid = res.json()["id"]
            res = client.post(f"{base}/threads/{tid}/messages/", json={"content": "hi"})
            assert res.status_code == 202, res.text
            turn_id = res.json()["turn"]["id"]
            with db_cursor() as cur:
                cur.execute(
                    "SELECT surface FROM assistant_turn WHERE id = %s", (turn_id,)
                )
                (surface,) = cur.fetchone()
            expected = (
                "desktop_assistant" if client is desktop_member else "web_assistant"
            )
            assert surface == expected, f"{client} stamped {surface!r}"
    finally:
        stub.stop()
