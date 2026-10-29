"""Oracle: provider response body-read failure retries on the OpenAI lane (PIDASHCONV-1491).

The stainless SDKs read the non-streaming response body inside the request
try block (``openai/_base_client.py`` ``client.send(stream=False)``; the
Anthropic client is the same), so a body that stalls past the timeout or a
connection reset mid-body raises ``APITimeoutError`` / ``APIConnectionError``,
is retried with the SDK's ``max_retries=2`` backoff, and finally surfaces as
pydantic-ai ``ModelAPIError`` (``"Request timed out."`` / ``"Connection
error."``). The run stores that text via ``cloud_agent.errors.sanitize_error``
with ``error_code`` ``provider_error``.

Each test seeds one ``agent_run`` row plus a Fernet-encrypted BYOK
``openai_compatible`` config pointing at the scripted mock below, publishes
``cloud_agent.run_agent_run`` through the live worker's channel (Celery wire
when a Django worker answers inspect, else a ``rust_job_queue`` row for
``pidash-api worker``), and asserts the terminal row:

- (a) a body stalled past the 60s model timeout: 3 attempts, then
  ``"Request timed out."``.
- (b) a connection reset mid-body: 3 attempts, then ``"Connection error."``.

The mock binds 127.0.0.1, so runs need ``ASSISTANT_BLOCK_PRIVATE_URLS``
(and the Rust ``CLOUD_AGENT_BLOCK_PRIVATE_URLS`` mirror) off (the guards
themselves are pinned by the assistant suite).
"""

from __future__ import annotations

import json
import os
import re
import socket
import struct
import threading
import time

import pytest

from _harness import broker_probe, rust_queue

from .conftest import FAILED, create_agent_run, wait_for_run

pytestmark = pytest.mark.contract

RUN_TASK = "cloud_agent.run_agent_run"
MODEL_NAME = "oracle-stub"
API_KEY = "test-key-123"

#: Declared body length the mocks never finish sending.
MOCK_CONTENT_LENGTH = 512

_PARTIAL_BODY = (
    b'{"id":"chatcmpl-reset","object":"chat.completion","choices":['
)

#: Stall longer than the 60s model request timeout
#: (``CLOUD_AGENT_MODEL_REQUEST_TIMEOUT_SECONDS`` default, read by both
#: stacks), with margin for a loaded worker.
STALL_SECS = 75.0

#: Settle delay after flushing headers (+ partial body) so the client has
#: drained them before the reset lands; the failure is then in the body
#: read on both stacks, never in the header read.
RST_SETTLE_SECS = 0.5

#: The stall test needs three 60s client timeouts plus SDK backoff.
STALL_WAIT_SECS = 240

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


class _BodyFaultProvider:
    """Mock OpenAI-compatible endpoint that faults the body on every request.

    A raw-socket server (no framework buffering surprises): it drains each
    request, records it, sends the 200 status line and headers with a
    ``Content-Length`` it never fulfils, then faults per ``mode``:

    - ``"stall"``: sends nothing more for ``STALL_SECS`` (the client times
      out mid-body).
    - ``"reset"``: flushes a partial body, lets the client drain it, then
      resets the connection (``SO_LINGER=0`` close).
    """

    def __init__(self, mode):
        assert mode in ("stall", "reset")
        self.mode = mode
        self.requests = []
        self._lock = threading.Lock()
        self._sock = None

    def start(self):
        stub = self
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sock.bind(("127.0.0.1", 0))
        sock.listen(16)
        sock.settimeout(1.0)
        self._sock = sock
        port = sock.getsockname()[1]

        def serve():
            while True:
                try:
                    conn, _ = sock.accept()
                except (OSError, socket.timeout):
                    if self._sock is None:
                        return
                    continue
                worker = threading.Thread(
                    target=stub._handle, args=(conn,), daemon=True
                )
                worker.start()

        threading.Thread(target=serve, daemon=True).start()
        return f"http://127.0.0.1:{port}/v1"

    def stop(self):
        sock, self._sock = self._sock, None
        if sock is not None:
            try:
                sock.close()
            except OSError:
                pass

    def _handle(self, conn):
        try:
            conn.settimeout(10.0)
            data = b""
            while b"\r\n\r\n" not in data:
                chunk = conn.recv(65536)
                if not chunk:
                    return
                data += chunk
            head, _, rest = data.partition(b"\r\n\r\n")
            match = re.search(rb"(?im)^content-length:\s*(\d+)", head)
            need = int(match.group(1)) if match else 0
            body = rest
            while len(body) < need:
                chunk = conn.recv(65536)
                if not chunk:
                    break
                body += chunk
            line = head.split(b"\r\n", 1)[0].decode("latin-1")
            try:
                payload = json.loads(body or b"{}")
            except ValueError:
                payload = {}
            with self._lock:
                self.requests.append({"request_line": line, "body": payload})
            conn.sendall(
                b"HTTP/1.1 200 OK\r\n"
                b"Content-Type: application/json\r\n"
                b"Content-Length: " + str(MOCK_CONTENT_LENGTH).encode() + b"\r\n"
                b"\r\n"
            )
            if self.mode == "stall":
                time.sleep(STALL_SECS)
                return
            conn.sendall(_PARTIAL_BODY)
            time.sleep(RST_SETTLE_SECS)
            conn.setsockopt(
                socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0)
            )
        except OSError:
            pass
        finally:
            try:
                conn.close()
            except OSError:
                pass


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


def test_openai_body_read_timeout_retries(db, broker, seeder, world):
    """(a) a body stalled past the timeout retries, then 'Request timed out.'."""
    stub = _BodyFaultProvider("stall")
    base_url = stub.start()
    try:
        _seed_llm_config(seeder, world["owner"]["id"], base_url)
        run = create_agent_run(
            seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
            prompt="contract provider body-stall run",
        )
        _publish(run["id"], broker)
        after = wait_for_run(db, run["id"], status=FAILED, timeout=STALL_WAIT_SECS)
        assert after["error_code"] == "provider_error"
        assert after["error"] == "Request timed out."
        assert len(stub.requests) == 3
        for seen in stub.requests:
            assert seen["request_line"].split(" ")[1].endswith("/chat/completions")
    finally:
        stub.stop()


def test_openai_body_reset_mid_body_retries(db, broker, seeder, world):
    """(b) a mid-body reset retries, then 'Connection error.'."""
    stub = _BodyFaultProvider("reset")
    base_url = stub.start()
    try:
        _seed_llm_config(seeder, world["owner"]["id"], base_url)
        run = create_agent_run(
            seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
            prompt="contract provider body-reset run",
        )
        _publish(run["id"], broker)
        after = wait_for_run(db, run["id"], status=FAILED)
        assert after["error_code"] == "provider_error"
        assert after["error"] == "Connection error."
        assert len(stub.requests) == 3
        for seen in stub.requests:
            assert seen["request_line"].split(" ")[1].endswith("/chat/completions")
    finally:
        stub.stop()
