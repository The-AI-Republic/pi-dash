"""Oracle: provider HTTP error text on the OpenAI-compatible lane (PIDASHCONV-1490).

The openai SDK (``_base_client._make_status_error_from_response`` +
``_client._make_status_error``) strips the error response text and unwraps
``body["error"]`` before pydantic-ai formats ``ModelHTTPError`` as
``status_code: {s}, model_name: {m}, body: {body}``; the run stores that
text via ``cloud_agent.errors.sanitize_error``. The Anthropic SDK strips
but never unwraps.

Each test seeds one ``agent_run`` row plus a Fernet-encrypted BYOK
``openai_compatible`` config pointing at the scripted mock below,
publishes ``cloud_agent.run_agent_run`` through the live worker's channel
(Celery wire when a Django worker answers inspect, else a
``rust_job_queue`` row for ``pidash-api worker``), and asserts the terminal
row:

- (a) a 401 ``{"error": {...}}`` body lands unwrapped on ``agent_run.error``.
- (b) a non-JSON 401 body lands stripped of trailing whitespace.

The mock binds 127.0.0.1, so runs need ``ASSISTANT_BLOCK_PRIVATE_URLS``
(and the Rust ``CLOUD_AGENT_BLOCK_PRIVATE_URLS`` mirror) off (the guards
themselves are pinned by the assistant suite).
"""

from __future__ import annotations

import json
import os
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

from _harness import broker_probe, rust_queue

from .conftest import FAILED, create_agent_run, wait_for_run

pytestmark = pytest.mark.contract

RUN_TASK = "cloud_agent.run_agent_run"
MODEL_NAME = "oracle-stub"
API_KEY = "test-key-123"
ERROR_INNER = {"message": "bad key", "type": "invalid_request_error", "code": None}

_channel = None


def _publish(run_id, broker):
    """Publish through the live worker's channel, detected once per session.

    A Django worker answers Celery inspect and consumes the redis wire;
    otherwise the Rust worker claims ``rust_job_queue`` rows. Exactly one
    worker runs in any contract run, so exactly one channel delivers.
    """
    global _channel
    if _channel is None:
        try:
            live = bool(broker_probe.registered_tasks(timeout=5.0))
        except Exception:
            live = False
        _channel = "celery" if live else "rust_queue"
        if _channel == "rust_queue":
            rust_queue.ensure_schema()
    if _channel == "celery":
        broker.publish(RUN_TASK, [run_id])
    else:
        rust_queue.publish(RUN_TASK, [run_id])


class _FailingProvider:
    """Mock OpenAI-compatible endpoint answering 401 to every request.

    ``payload`` is the exact response body bytes; every request is
    recorded. A 401 is retried by neither stack (SDK ``_should_retry``
    skips it; the agent graph only retries ``UnexpectedModelBehavior``),
    so each test sees exactly one request.
    """

    def __init__(self, payload, *, content_type="application/json"):
        self.payload = payload
        self.content_type = content_type
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
                    payload = json.loads(raw or b"{}")
                except ValueError:
                    payload = {}
                stub.requests.append({"path": self.path, "body": payload})
                self.send_response(401)
                self.send_header("Content-Type", stub.content_type)
                self.send_header("Content-Length", str(len(stub.payload)))
                self.end_headers()
                self.wfile.write(stub.payload)

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self._server.daemon_threads = True
        port = self._server.server_address[1]
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()
        return f"http://127.0.0.1:{port}/v1"

    def stop(self):
        if self._server is not None:
            self._server.shutdown()
            self._server.server_close()
            self._server = None


def _seed_llm_config(seeder, user_id, base_url):
    """Seed a decryptable BYOK config pointing at the mock endpoint."""
    from cryptography.fernet import Fernet

    key = os.environ["ASSISTANT_ENCRYPTION_KEY"].encode("utf-8")
    token = Fernet(key).encrypt(API_KEY.encode("utf-8"))
    seeder.db.execute(
        """INSERT INTO assistant_user_llm_config
           (provider_kind, base_url, model_name, api_key_encrypted,
            last_verified_at, created_at, updated_at, user_id)
           VALUES ('openai_compatible',%s,%s,%s,NULL,now(),now(),%s)""",
        (base_url, MODEL_NAME, token, user_id),
    )
    row_id = seeder.db.fetchval(
        "SELECT id FROM assistant_user_llm_config WHERE user_id = %s", (user_id,)
    )
    seeder.track.add("assistant_user_llm_config", row_id)
    return {"id": row_id}


def test_openai_http_error_unwraps_error_envelope(db, broker, seeder, world):
    """(a) the {'error': ...} envelope is unwrapped before the run stores it."""
    stub = _FailingProvider(json.dumps({"error": ERROR_INNER}).encode())
    base_url = stub.start()
    try:
        _seed_llm_config(seeder, world["owner"]["id"], base_url)
        run = create_agent_run(
            seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
            prompt="contract provider-error run",
        )
        _publish(run["id"], broker)
        after = wait_for_run(db, run["id"], status=FAILED)
        assert after["error_code"] == "provider_error"
        assert after["error"] == (
            "status_code: 401, model_name: oracle-stub, body: "
            "{'message': 'bad key', 'type': 'invalid_request_error', 'code': None}"
        )
        assert len(stub.requests) == 1
        assert stub.requests[0]["path"].endswith("/chat/completions")
    finally:
        stub.stop()


def test_openai_http_error_strips_text_body(db, broker, seeder, world):
    """(b) a non-JSON 401 body lands stripped of trailing whitespace."""
    stub = _FailingProvider(b"upstream exploded\n\n", content_type="text/plain")
    base_url = stub.start()
    try:
        _seed_llm_config(seeder, world["owner"]["id"], base_url)
        run = create_agent_run(
            seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
            prompt="contract provider-error run",
        )
        _publish(run["id"], broker)
        after = wait_for_run(db, run["id"], status=FAILED)
        assert after["error_code"] == "provider_error"
        assert after["error"] == (
            "status_code: 401, model_name: oracle-stub, body: upstream exploded"
        )
        assert len(stub.requests) == 1
        assert stub.requests[0]["path"].endswith("/chat/completions")
    finally:
        stub.stop()
