# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Multi-model BYOK on ``/config/`` (NEWPIDASH-392, OSS-2, DIFF-64).

Covers ``PUT .../config/`` with a ``models`` list (validation, the
per-ID save-time smoke test, ``save_unverified``, ``models_required``
and ``model_test_failed``), the widened ``GET`` shape, and
``POST .../config/test/`` with a ``{"model": ...}`` body. The list
validation and smoke test are new-system-only: the Django arm pins the
old shapes (``expect("DIFF-64", ...)``), including how Django answers
the unknown ``models`` field.
"""

from __future__ import annotations

import os
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from _harness.diffs import expect, is_rust

from .conftest import PUBLIC_BASE

CONFIG_URL = "/api/users/me/ai-assistant/config/"
TEST_URL = "/api/users/me/ai-assistant/config/test/"
DEEPSEEK_BASE = "http://api.deepseek.com/v1"
PROVIDER_PROXY = "http://127.0.0.1:18097"


def _require_proxy_env():
    proxy_env = os.environ.get("HTTP_PROXY") or os.environ.get("http_proxy")
    assert proxy_env == PROVIDER_PROXY, (
        "assistant contract runs must point the backend HTTP_PROXY at "
        f"{PROVIDER_PROXY} so the model stub can answer"
    )


class _ModelStubProxy:
    """Per-model provider stub behind ``HTTP_PROXY`` (see
    ``test_model_selection.py``). ``responses`` maps model IDs to
    ``(status, body)`` and is mutable, so a test can flip a model's
    answer between save and test.
    """

    def __init__(self):
        import json

        stub = self
        self._json = json
        self.responses = {
            "ok-a": (200, {"id": "stub", "object": "chat.completion", "created": 0, "model": "stub-model", "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}]}),
            "ok-b": (200, {"id": "stub", "object": "chat.completion", "created": 0, "model": "stub-model", "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}]}),
            "flip-model": (200, {"id": "stub", "object": "chat.completion", "created": 0, "model": "stub-model", "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}]}),
            "gone-model": (404, {"error": {"message": "model not found"}}),
            "bad-model": (401, {"error": {"message": "Incorrect API key"}}),
        }
        self._server = None
        self._thread = None
        # When set, every probe answers 401 (a rotated key the
        # provider rejects), whatever the model ID.
        self.fail_all = False

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
                if stub.fail_all:
                    status, payload = (401, {"error": {"message": "Incorrect API key"}})
                else:
                    status, payload = stub.responses.get(model, (500, {"error": "boom"}))
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


def _put_key(client, models, base_url=DEEPSEEK_BASE, **extra):
    # `model_name` rides along so the Django arm (which ignores the
    # unknown `models` field) still saves a valid row.
    body = {
        "provider_kind": "openai_compatible",
        "base_url": base_url,
        "model_name": "m",
        "models": models,
        "api_key": "test-key-123",
        **extra,
    }
    return client.put(CONFIG_URL, json=body)


def test_byok_models_validation(world, member):
    """List rules: trim, dedupe, 255 chars, 20 max, empty rejected.

    Django ignores the unknown field (probed live: 200, old shape), so
    the Django arm pins the tolerant old behaviour while the Rust arm
    pins the new rules.
    """
    _require_proxy_env()
    stub = _ModelStubProxy().start()
    try:
        # Trim + dedupe: first occurrence wins, empties dropped.
        res = _put_key(member, ["  ok-a  ", "ok-b", "ok-a", "   "])
        assert res.status_code == expect("DIFF-64", django=200, rust=200), res.text
        if is_rust():
            body = res.json()["models"]
            assert [entry["model"] for entry in body] == ["ok-a", "ok-b"]
            assert all(entry["status"] == "verified" for entry in body)
            assert member.get(CONFIG_URL).json()["models"] == ["ok-a", "ok-b"]
        else:
            # Old shape, no models keys.
            assert "models" not in res.json()

        # One ID over 255 characters is a field error on Rust.
        res = _put_key(member, ["x" * 256])
        assert res.status_code == expect("DIFF-64", django=200, rust=400), res.text
        if is_rust():
            assert "models" in res.json()

        # More than 20 IDs is a field error on Rust.
        res = _put_key(member, [f"m{i}" for i in range(21)])
        assert res.status_code == expect("DIFF-64", django=200, rust=400), res.text
        if is_rust():
            assert "models" in res.json()

        # An empty list is rejected as models_required on Rust.
        res = _put_key(member, [])
        assert res.status_code == expect("DIFF-64", django=200, rust=400), res.text
        if is_rust():
            assert res.json()["error"] == "models_required"

        # A non-list is a field error on Rust.
        res = _put_key(member, "ok-a")
        assert res.status_code == expect("DIFF-64", django=200, rust=400), res.text
        if is_rust():
            assert "models" in res.json()
    finally:
        stub.stop()


def test_byok_smoke_verified_saves_and_stamps(world, member):
    """All-verified saves persist the models, the verified map and the
    legacy stamp. Django saves the row shape without probing."""
    _require_proxy_env()
    stub = _ModelStubProxy().start()
    try:
        res = _put_key(member, ["ok-a", "ok-b"])
        assert res.status_code == 200, res.text
        if not is_rust():
            # Old shape, old behaviour: saved without any probe.
            assert "models" not in res.json()
            return
        assert res.json() == {
            "models": [
                {"model": "ok-a", "status": "verified"},
                {"model": "ok-b", "status": "verified"},
            ]
        }
        body = member.get(CONFIG_URL).json()
        assert body["models"] == ["ok-a", "ok-b"]
        assert body["model_name"] == "ok-a"
        assert set(body["model_verified_at"]) == {"ok-a", "ok-b"}
        assert body["last_verified_at"] is not None
    finally:
        stub.stop()


def test_byok_smoke_failed_rejects_without_saving(world, member):
    """A failed ID rejects the whole write (nothing saved). Django
    saves without probing."""
    _require_proxy_env()
    stub = _ModelStubProxy().start()
    try:
        res = _put_key(member, ["ok-a", "gone-model"])
        if not is_rust():
            assert res.status_code == 200, res.text
            return
        assert res.status_code == 400, res.text
        body = res.json()
        assert body["error"] == "model_test_failed"
        assert body["models"] == [
            {"model": "ok-a", "status": "verified"},
            {
                "model": "gone-model",
                "status": "failed",
                "code": "model_invalid",
                "detail": "Model not accepted by the provider.",
            },
        ]
        # Nothing was saved.
        saved = member.get(CONFIG_URL).json()
        assert saved["has_api_key"] is False
        assert saved["models"] == []
    finally:
        stub.stop()


def test_byok_smoke_unverified_needs_save_unverified(world, member):
    """Unreachable IDs save only with the opt-in flag. Django saves
    without probing."""
    _require_proxy_env()
    stub = _ModelStubProxy().start()
    try:
        # The stub 500s unknown models: fast `provider_unreachable`.
        res = _put_key(member, ["down-model"])
        if not is_rust():
            assert res.status_code == 200, res.text
            return
        assert res.status_code == 400, res.text
        assert res.json()["error"] == "model_test_failed"
        assert res.json()["models"][0]["status"] == "unverified"

        res = _put_key(member, ["down-model"], save_unverified=True)
        assert res.status_code == 200, res.text
        assert res.json()["models"][0]["status"] == "unverified"
        saved = member.get(CONFIG_URL).json()
        assert saved["models"] == ["down-model"]
        assert saved["model_verified_at"] == {}
        assert saved["last_verified_at"] is None
    finally:
        stub.stop()


def test_byok_key_rotation_retests_every_id(world, member):
    """Rotating the key retests every stored ID: a rejected key fails
    the save (nothing stored) instead of keeping the old stamps.
    Django saves without probing."""
    _require_proxy_env()
    stub = _ModelStubProxy().start()
    try:
        res = _put_key(member, ["ok-a", "ok-b"])
        assert res.status_code == 200, res.text
        if not is_rust():
            return
        assert {entry["model"] for entry in res.json()["models"]} == {"ok-a", "ok-b"}

        # Same models, fresh key, provider now rejects everything: the
        # save must probe both IDs and reject without saving.
        stub.fail_all = True
        res = _put_key(member, ["ok-a", "ok-b"], api_key="rotated-key-456")
        assert res.status_code == 400, res.text
        body = res.json()
        assert body["error"] == "model_test_failed"
        assert [entry["model"] for entry in body["models"]] == ["ok-a", "ok-b"]
        assert all(entry["status"] == "failed" for entry in body["models"])
        # Nothing was saved: the old models and stamps survive.
        saved = member.get(CONFIG_URL).json()
        assert saved["models"] == ["ok-a", "ok-b"]
        assert set(saved["model_verified_at"]) == {"ok-a", "ok-b"}

        # Rotating back to a key the provider accepts saves with fresh
        # stamps for every ID.
        stub.fail_all = False
        res = _put_key(member, ["ok-a", "ok-b"], api_key="rotated-key-789")
        assert res.status_code == 200, res.text
        assert all(entry["status"] == "verified" for entry in res.json()["models"])
        assert set(member.get(CONFIG_URL).json()["model_verified_at"]) == {"ok-a", "ok-b"}
    finally:
        stub.stop()


def test_byok_model_test_endpoint(world, member):
    """``POST .../config/test/ {"model"}`` tests one stored ID and
    updates `model_verified_at`; the no-body form keeps the legacy
    `{ok}` shape on both backends. Django ignores the body and probes
    the row's single model."""
    _require_proxy_env()
    stub = _ModelStubProxy().start()
    try:
        # No config: both backends answer the missing-config shape.
        res = member.post(TEST_URL, json={"model": "ok-a"})
        assert res.json()["ok"] is False
        assert res.json()["error_code"] == "llm_config_missing"

        _put_key(member, ["ok-a", "flip-model"], model_name="ok-a")
        res = member.post(TEST_URL, json={"model": "ok-a"})
        if not is_rust():
            # Django ignores the body and probes the row's model.
            assert res.json()["ok"] is True
            return
        assert res.status_code == 200, res.text
        assert res.json() == {"model": "ok-a", "status": "verified"}
        saved = member.get(CONFIG_URL).json()
        assert set(saved["model_verified_at"]) >= {"ok-a", "flip-model"}

        # A stored ID that starts failing reports failed and drops its
        # stamp (the stub flips its answer after the save).
        stub.responses["flip-model"] = (404, {"error": {"message": "model not found"}})
        res = member.post(TEST_URL, json={"model": "flip-model"})
        assert res.status_code == 200, res.text
        assert res.json()["status"] == "failed"
        assert res.json()["code"] == "model_invalid"
        assert "flip-model" not in member.get(CONFIG_URL).json()["model_verified_at"]

        # An ID the key does not hold is rejected.
        res = member.post(TEST_URL, json={"model": "nope"})
        assert res.status_code == 400
        assert res.json()["error"] == "model_not_available"
    finally:
        stub.stop()
