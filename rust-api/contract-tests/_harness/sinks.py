"""Local stub sinks: capture outbound side effects without touching Django."""
from __future__ import annotations

import json
import threading
from email import message_from_bytes
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

from . import config
from .db import wait_for_condition


class RecordingHandler(BaseHTTPRequestHandler):
    requests: list[dict] = []

    def _record(self) -> None:
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b""
        type(self).requests.append(
            {
                "method": self.command,
                "path": self.path,
                "headers": dict(self.headers),
                "body": raw,
            }
        )
        payload = json.dumps({"ok": True}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    do_GET = _record
    do_POST = _record
    do_PUT = _record
    do_PATCH = _record
    do_DELETE = _record

    def log_message(self, *args: object) -> None:  # keep test output clean
        pass


class StubSink:
    """A tiny HTTP server that records every request and answers 200 {}."""

    def __init__(self) -> None:
        self._server = ThreadingHTTPServer(("127.0.0.1", 0), RecordingHandler)
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        RecordingHandler.requests = []

    @property
    def url(self) -> str:
        host, port = self._server.server_address
        return f"http://{host}:{port}"

    def __enter__(self) -> "StubSink":
        self._thread.start()
        return self

    def __exit__(self, *args: object) -> None:
        self._server.shutdown()
        self._thread.join(timeout=5)
        self._server.server_close()

    @property
    def requests(self) -> list[dict]:
        return list(RecordingHandler.requests)


# -- SMTP + webhook sinks (PIDASHCONV-81). Kept alongside StubSink above
# (never a fork): distinct classes for mail / webhook side effects.
class SmtpSink:
    """In-memory SMTP server. `messages` is a list of dicts
    {peer, mail_from, rcpt_tos, data: str}."""

    def __init__(self):
        import socket

        from aiosmtpd.controller import Controller

        self.messages: list[dict] = []
        # aiosmtpd cannot start on port 0 itself; pre-allocate an ephemeral
        # loopback port and hand it over explicitly.
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.bind(("127.0.0.1", 0))
            free_port = s.getsockname()[1]
        outer = self

        class Handler:
            async def handle_DATA(self, server, session, envelope):
                outer.messages.append(
                    {
                        "peer": session.peer,
                        "mail_from": envelope.mail_from,
                        "rcpt_tos": list(envelope.rcpt_tos),
                        "data": envelope.content.decode("utf8", "replace"),
                    }
                )
                return "250 OK"

        self._controller = Controller(Handler(), hostname="127.0.0.1", port=free_port)
        self._controller.start()
        self.host, self.port = "127.0.0.1", free_port

    def stop(self):
        self._controller.stop()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.stop()


class WebhookSink:
    """In-memory HTTP server. `requests` is a list of dicts
    {method, path, headers, body: bytes, json}."""

    def __init__(self):
        outer = self
        self.requests: list[dict] = []

        class Handler(BaseHTTPRequestHandler):
            def _capture(self):
                length = int(self.headers.get("Content-Length", 0) or 0)
                body = self.rfile.read(length) if length else b""
                try:
                    parsed = json.loads(body) if body else None
                except ValueError:
                    parsed = None
                outer.requests.append(
                    {
                        "method": self.command,
                        "path": self.path,
                        "headers": dict(self.headers),
                        "body": body,
                        "json": parsed,
                    }
                )
                payload = b'{"ok": true}'
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            do_POST = _capture
            do_PUT = _capture
            do_PATCH = _capture

            def log_message(self, *args):
                pass

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.host, self.port = self._server.server_address[:2]
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()

    @property
    def url(self) -> str:
        return f"http://{self.host}:{self.port}"

    def stop(self):
        self._server.shutdown()
        self._thread.join(timeout=10)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.stop()


# --- PIDASHCONV-83: worker-plane side-effect sinks ---
# PIDASHCONV-81 (above) owns the canonical ``SmtpSink``/``WebhookSink``
# names (ephemeral-port, context-manager style, with live consumers), so
# this block's fixed-port variants for the worker-plane oracle (Django
# EMAIL settings point at a fixed host:port; webhooks need fail_next
# injection) live under ``Worker*`` names with ``smtp_sink``/
# ``webhook_sink`` session fixtures. Extend, never fork.
class WorkerSmtpSink:
    """Collects every message delivered to it. Plain SMTP, no auth/TLS."""

    def __init__(self):
        self.messages = []
        self._lock = threading.Lock()
        self._controller = None

    def start(self, host: str, port: int) -> None:
        from aiosmtpd.controller import Controller

        sink = self

        class Handler:
            async def handle_DATA(self, server, session, envelope):
                parsed = message_from_bytes(envelope.content)
                with sink._lock:
                    sink.messages.append(
                        {
                            "mail_from": envelope.mail_from,
                            "rcpt_tos": list(envelope.rcpt_tos),
                            "subject": parsed.get("Subject"),
                            "raw": envelope.content.decode("utf-8", "replace"),
                        }
                    )
                return "250 OK"

        self._controller = Controller(Handler(), hostname=host, port=port)
        self._controller.start()

    def stop(self) -> None:
        if self._controller is not None:
            self._controller.stop()
            self._controller = None

    def clear(self) -> None:
        with self._lock:
            self.messages = []

    def snapshot(self) -> list:
        with self._lock:
            return list(self.messages)

    def wait_for_count(self, n: int, what: str = "smtp deliveries") -> list:
        return wait_for_condition(
            lambda: self.snapshot() if len(self.snapshot()) >= n else None,
            what=what,
        )


class WorkerWebhookSinkHandler(BaseHTTPRequestHandler):
    sink = None  # set per server instance
    failures_remaining = 0

    def log_message(self, *args):  # keep test output clean
        pass

    def _reply(self, code: int, payload: dict) -> None:
        body = json.dumps(payload).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(length) if length else b""
        with self.sink._lock:
            self.sink.deliveries.append(
                {
                    "path": self.path,
                    "headers": dict(self.headers),
                    "body": raw.decode("utf-8", "replace"),
                }
            )
        if type(self).failures_remaining > 0:
            type(self).failures_remaining -= 1
            self._reply(500, {"ok": False})
        else:
            self._reply(200, {"ok": True})


class WorkerWebhookSink:
    """Collects webhook POSTs; ``fail_next(n)`` forces n 500s (retry tests)."""

    def __init__(self):
        self.deliveries = []
        self._lock = threading.Lock()
        self._server = None
        self._thread = None

    def start(self, base_url: str) -> None:
        from urllib.parse import urlparse

        parts = urlparse(base_url)
        handler = type(
            "BoundHandler",
            (WorkerWebhookSinkHandler,),
            {"sink": self, "failures_remaining": 0},
        )
        self._handler = handler
        self._server = ThreadingHTTPServer((parts.hostname, parts.port), handler)
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()

    def stop(self) -> None:
        if self._server is not None:
            self._server.shutdown()
            self._thread.join(timeout=10)
            self._server.server_close()
            self._server = None

    def clear(self) -> None:
        with self._lock:
            self.deliveries = []
        self._handler.failures_remaining = 0

    def fail_next(self, n: int) -> None:
        self._handler.failures_remaining = n

    def snapshot(self) -> list:
        with self._lock:
            return list(self.deliveries)

    def wait_for_count(self, n: int, what: str = "webhook deliveries") -> list:
        return wait_for_condition(
            lambda: self.snapshot() if len(self.snapshot()) >= n else None,
            what=what,
        )


@pytest.fixture(scope="session")
def smtp_sink():
    sink = WorkerSmtpSink()
    sink.start(config.SMTP_SINK_HOST, config.SMTP_SINK_PORT)
    yield sink
    sink.stop()


@pytest.fixture(scope="session")
def webhook_sink():
    sink = WorkerWebhookSink()
    sink.start(config.WEBHOOK_SINK_BASE)
    yield sink
    sink.stop()
