"""Local stub sinks: capture outbound side effects without touching Django."""

from __future__ import annotations

import base64
import datetime
import json
import select
import socket
import ssl
import threading
import time
from email import message_from_bytes
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit

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


class FixedResponseStub:
    """HTTP stub answering every request with one fixed response.

    Unlike :class:`StubSink` (always 200 ``{"ok": true}``), the status,
    content type and body are caller-chosen (first use: PIDASHCONV-809,
    stubbing the GitHub releases endpoint). `requests` records
    ``{method, path, headers, body}`` per hit.
    """

    def __init__(
        self,
        *,
        status: int = 200,
        body: bytes = b"",
        content_type: str = "application/json",
    ):
        outer = self
        self.status = status
        self.body = body
        self.content_type = content_type
        self.requests: list[dict] = []

        class Handler(BaseHTTPRequestHandler):
            def _answer(self) -> None:
                length = int(self.headers.get("Content-Length") or 0)
                raw = self.rfile.read(length) if length else b""
                outer.requests.append(
                    {
                        "method": self.command,
                        "path": self.path,
                        "headers": dict(self.headers),
                        "body": raw,
                    }
                )
                self.send_response(outer.status)
                self.send_header("Content-Type", outer.content_type)
                self.send_header("Content-Length", str(len(outer.body)))
                self.end_headers()
                self.wfile.write(outer.body)

            do_GET = _answer
            do_POST = _answer
            do_PUT = _answer
            do_PATCH = _answer
            do_DELETE = _answer

            def log_message(self, *args: object) -> None:  # keep test output clean
                pass

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)

    @property
    def url(self) -> str:
        host, port = self._server.server_address
        return f"http://{host}:{port}"

    def __enter__(self) -> "FixedResponseStub":
        self._thread.start()
        return self

    def __exit__(self, *args: object) -> None:
        self._server.shutdown()
        self._thread.join(timeout=5)
        self._server.server_close()


class WildcardHttpProbe:
    """A title/favicon page bound to a wildcard address.

    The SSRF guard test needs the worker itself to reach the listener by
    connecting to ``0.0.0.0`` or ``::``. A loopback-bound sink can never
    distinguish "guard refused" from "connection refused", so this stub
    binds the wildcard address and answers a distinctive title and icon.
    ``family`` is ``socket.AF_INET`` (``0.0.0.0``) or ``socket.AF_INET6``
    (``::``); :attr:`url` carries the brackets IPv6 URLs require.
    """

    TITLE = "SSRF wildcard probe"
    FAVICON = b"wildcard-probe-favicon"

    def __init__(self, family: int = socket.AF_INET) -> None:
        outer = self

        class Handler(BaseHTTPRequestHandler):
            def _answer(self, body: bytes, content_type: str) -> None:
                self.send_response(200)
                self.send_header("Content-Type", content_type)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                if self.command != "HEAD":
                    self.wfile.write(body)

            def do_GET(self) -> None:
                if urlsplit(self.path).path == "/favicon.ico":
                    self._answer(outer.FAVICON, "image/x-icon")
                    return
                body = (
                    "<html><head><title>"
                    f"{outer.TITLE}"
                    '</title><link rel="icon" href="/favicon.ico"></head></html>'
                ).encode()
                self._answer(body, "text/html")

            do_HEAD = do_GET

            def log_message(self, *args: object) -> None:
                pass

        class Server(ThreadingHTTPServer):
            address_family = family

        host = "::" if family == socket.AF_INET6 else "0.0.0.0"
        self._server = Server((host, 0), Handler)
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self.host, self.port = self._server.server_address[:2]

    @property
    def url(self) -> str:
        host = f"[{self.host}]" if ":" in self.host else self.host
        return f"http://{host}:{self.port}/"

    def __enter__(self) -> "WildcardHttpProbe":
        self._thread.start()
        return self

    def __exit__(self, *args: object) -> None:
        self._server.shutdown()
        self._thread.join(timeout=5)
        self._server.server_close()


class SlowDripSink:
    """A page dripped across two timed gaps (PIDASHCONV-1669).

    Every GET waits ``first_gap`` seconds, sends the headers plus the
    first half of ``body``, waits ``second_gap`` seconds, then sends the
    rest. A client with a per-read timeout larger than either gap reads
    the whole page whatever the total; a client with a total deadline
    shorter than the sum fails. HEAD serves the same headers without a
    body (favicon-probe shape); ``/favicon.ico`` answers immediately.
    """

    BODY = (
        b"<html><head><title>Slow drip oracle</title>"
        b'<link rel="icon" href="/favicon.ico"></head>'
        b"<body>hello</body></html>"
    )
    FAVICON = b"slow-drip-sink-favicon"

    def __init__(
        self,
        first_gap: float = 0.6,
        second_gap: float = 0.6,
        body: bytes | None = None,
    ) -> None:
        outer = self
        self.first_gap = first_gap
        self.second_gap = second_gap
        self.body = self.BODY if body is None else body

        class Handler(BaseHTTPRequestHandler):
            def _answer(self, payload: bytes, content_type: str, gaps: bool) -> None:
                if gaps:
                    time.sleep(outer.first_gap)
                self.send_response(200)
                self.send_header("Content-Type", content_type)
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                if self.command == "HEAD":
                    return
                half = len(payload) // 2
                self.wfile.write(payload[:half])
                self.wfile.flush()
                if gaps:
                    time.sleep(outer.second_gap)
                self.wfile.write(payload[half:])
                self.wfile.flush()

            def do_GET(self) -> None:
                if urlsplit(self.path).path == "/favicon.ico":
                    self._answer(outer.FAVICON, "image/x-icon", gaps=False)
                    return
                self._answer(outer.body, "text/html", gaps=True)

            do_HEAD = do_GET

            def log_message(self, *args: object) -> None:
                pass

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)

    @property
    def url(self) -> str:
        host, port = self._server.server_address
        return f"http://{host}:{port}"

    def __enter__(self) -> "SlowDripSink":
        self._thread.start()
        return self

    def __exit__(self, *args: object) -> None:
        self._server.shutdown()
        self._thread.join(timeout=5)
        self._server.server_close()


class RoutingStub:
    """HTTP stub dispatching fixed responses by (method, path).

    First use: PIDASHCONV-1089, stubbing the Gitea OAuth endpoints
    (``GITEA_HOST`` points at :attr:`url`). ``routes`` maps
    ``(METHOD, path)`` (path without query string) to
    ``(status, body)``; ``body`` may be bytes, str, or a JSON-serializable
    value (dict/list, served as ``application/json``). A route may also be
    a ``(status, body, content_type)`` triple to override the served
    ``Content-Type`` (PIDASHCONV-1321: the stubbed avatar bytes must read
    as an image or both backends reject the download). Unmapped paths
    answer 404. Every hit records ``{method, path, headers, body}`` in
    :attr:`requests` (full path, query string included).
    """

    def __init__(self, routes: dict[tuple[str, str], tuple[int, object]]) -> None:
        outer = self
        self.requests: list[dict] = []
        normalized: dict[tuple[str, str], tuple[int, bytes, str]] = {}
        for (method, path), route in routes.items():
            if len(route) == 3:
                status, body, content_type = route
            else:
                status, body = route
                content_type = None
            if isinstance(body, (dict, list)):
                payload = json.dumps(body).encode()
                content_type = content_type or "application/json"
            elif isinstance(body, str):
                payload = body.encode()
                content_type = content_type or "application/json"
            else:
                payload = bytes(body)
                content_type = content_type or "application/json"
            normalized[(method, path)] = (status, payload, content_type)

        class Handler(BaseHTTPRequestHandler):
            def _answer(self) -> None:
                length = int(self.headers.get("Content-Length") or 0)
                raw = self.rfile.read(length) if length else b""
                outer.requests.append(
                    {
                        "method": self.command,
                        "path": self.path,
                        "headers": dict(self.headers),
                        "body": raw,
                    }
                )
                route = normalized.get((self.command, urlsplit(self.path).path))
                if route is None:
                    self.send_response(404)
                    self.send_header("Content-Type", "text/plain")
                    self.send_header("Content-Length", "9")
                    self.end_headers()
                    self.wfile.write(b"not found")
                    return
                status, payload, content_type = route
                self.send_response(status)
                self.send_header("Content-Type", content_type)
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            do_GET = _answer
            do_POST = _answer
            do_PUT = _answer
            do_PATCH = _answer
            do_DELETE = _answer

            def log_message(self, *args: object) -> None:  # keep test output clean
                pass

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)

    @property
    def url(self) -> str:
        host, port = self._server.server_address
        return f"http://{host}:{port}"

    def __enter__(self) -> "RoutingStub":
        self._thread.start()
        return self

    def __exit__(self, *args: object) -> None:
        self._server.shutdown()
        self._thread.join(timeout=5)
        self._server.server_close()


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


class ChunkedKeepAliveSink:
    """Webhook sink answering ``Transfer-Encoding: chunked`` and staying open.

    Sends ``2\\r\\nok\\r\\n0\\r\\n\\r\\n`` and then holds the connection for
    longer than the webhook client's 30 s read timeout. ``requests`` (the
    Django oracle) returns at the terminal chunk; a client that instead
    reads to EOF hangs and records a false failure.
    """

    def __init__(self, hold_seconds: float = 45.0):
        self.hold_seconds = hold_seconds
        self.deliveries = []
        self._lock = threading.Lock()
        self._server = None
        self._thread = None

    def start(self) -> None:
        outer = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def do_POST(self):
                length = int(self.headers.get("Content-Length", 0))
                raw = self.rfile.read(length) if length else b""
                with outer._lock:
                    outer.deliveries.append(
                        {
                            "path": self.path,
                            "headers": dict(self.headers),
                            "body": raw.decode("utf-8", "replace"),
                        }
                    )
                self.send_response(200)
                self.send_header("Content-Type", "text/plain")
                self.send_header("Transfer-Encoding", "chunked")
                self.send_header("Connection", "keep-alive")
                self.end_headers()
                self.wfile.write(b"2\r\nok\r\n0\r\n\r\n")
                self.wfile.flush()
                time.sleep(outer.hold_seconds)

            def log_message(self, *args):
                pass

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()

    @property
    def url(self) -> str:
        host, port = self._server.server_address
        return f"http://{host}:{port}"

    def stop(self) -> None:
        if self._server is not None:
            self._server.shutdown()
            self._thread.join(timeout=10)
            self._server.server_close()
            self._server = None

    def clear(self) -> None:
        with self._lock:
            self.deliveries = []

    def snapshot(self) -> list:
        with self._lock:
            return list(self.deliveries)

    def wait_for_count(self, n: int, what: str = "chunked webhook deliveries") -> list:
        return wait_for_condition(
            lambda: self.snapshot() if len(self.snapshot()) >= n else None,
            what=what,
        )


class ChunkedStreamSink:
    """Webhook sink streaming a large chunked body in paced chunks.

    Answers every POST with ``Transfer-Encoding: chunked`` and writes
    ``chunk_count`` chunks of ``chunk_size`` bytes, pausing
    ``pace_seconds`` between chunks so socket reads stay small, then
    the terminal chunk and close. A client that re-decodes the whole
    buffered body after every read (PIDASHCONV-1673) spends quadratic
    time here; an incremental decoder finishes near the pacing floor.
    """

    def __init__(
        self,
        chunk_count: int = 8192,
        chunk_size: int = 8192,
        pace_seconds: float = 0.0002,
        fill: bytes = b"x",
    ) -> None:
        self.chunk_count = chunk_count
        self.chunk_size = chunk_size
        self.pace_seconds = pace_seconds
        self.fill = fill
        self.deliveries: list = []
        self._lock = threading.Lock()
        self._server = None
        self._thread = None

    @property
    def total_bytes(self) -> int:
        return self.chunk_count * self.chunk_size

    def start(self) -> None:
        outer = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def do_POST(self):
                length = int(self.headers.get("Content-Length", 0))
                raw = self.rfile.read(length) if length else b""
                with outer._lock:
                    outer.deliveries.append(
                        {
                            "path": self.path,
                            "headers": dict(self.headers),
                            "body": raw.decode("utf-8", "replace"),
                        }
                    )
                payload = outer.fill * outer.chunk_size
                self.send_response(200)
                self.send_header("Content-Type", "text/plain")
                self.send_header("Transfer-Encoding", "chunked")
                self.send_header("Connection", "close")
                self.end_headers()
                size_line = format(outer.chunk_size, "x").encode() + b"\r\n"
                for _ in range(outer.chunk_count):
                    self.wfile.write(size_line)
                    self.wfile.write(payload)
                    self.wfile.write(b"\r\n")
                    self.wfile.flush()
                    time.sleep(outer.pace_seconds)
                self.wfile.write(b"0\r\n\r\n")
                self.wfile.flush()

            def log_message(self, *args):
                pass

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()

    @property
    def url(self) -> str:
        host, port = self._server.server_address
        return f"http://{host}:{port}"

    def stop(self) -> None:
        if self._server is not None:
            self._server.shutdown()
            self._thread.join(timeout=30)
            self._server.server_close()
            self._server = None

    def clear(self) -> None:
        with self._lock:
            self.deliveries = []

    def snapshot(self) -> list:
        with self._lock:
            return list(self.deliveries)

    def wait_for_count(self, n: int, what: str = "streamed webhook deliveries") -> list:
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


class WorkerPosthogSinkHandler(BaseHTTPRequestHandler):
    sink = None  # set per server instance

    def log_message(self, *args):  # keep test output clean
        pass

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(length) if length else b""
        try:
            parsed = json.loads(raw) if raw else None
        except ValueError:
            parsed = None
        with self.sink._lock:
            self.sink.captures.append(
                {
                    "path": self.path,
                    "headers": dict(self.headers),
                    "body": raw.decode("utf-8", "replace"),
                    "json": parsed,
                }
            )
        payload = b'{"status": 1}'
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


class WorkerPosthogSink:
    """Collects PostHog /batch/ captures (PIDASHCONV-1107).

    Same fixed-port session shape as WorkerWebhookSink: the worker
    under test POSTs here because its POSTHOG_HOST points at
    POSTHOG_SINK_BASE. Answers 200 {"status": 1} like the real API.
    """

    def __init__(self):
        self.captures = []
        self._lock = threading.Lock()
        self._server = None
        self._thread = None

    def start(self, base_url: str) -> None:
        from urllib.parse import urlparse

        parts = urlparse(base_url)
        handler = type(
            "BoundPosthogHandler",
            (WorkerPosthogSinkHandler,),
            {"sink": self},
        )
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
            self.captures = []

    def snapshot(self) -> list:
        with self._lock:
            return list(self.captures)

    def wait_for_count(self, n: int, what: str = "posthog captures") -> list:
        return wait_for_condition(
            lambda: self.snapshot() if len(self.snapshot()) >= n else None,
            what=what,
        )


@pytest.fixture(scope="session")
def posthog_sink():
    sink = WorkerPosthogSink()
    sink.start(config.POSTHOG_SINK_BASE)
    yield sink
    sink.stop()


class LiveDocStub:
    """Stub live-document conversion server for page-write tests (PIDASHCONV-1503).

    Page writes convert through ``POST {LIVE_URL}/convert-document/``
    (``pi_dash.utils.live_document``); without a live server both backends
    503 and nothing is written. The stub binds the fixed base in
    ``LIVE_DOC_STUB_BASE`` — the backend under test points ``LIVE_BASE_URL``
    at the same base — and answers every POST with one fixed usable
    document (non-empty binary, empty body HTML, empty JSON).

    Context-manager scoped (one conversion window per test), deliberately
    NOT a session fixture: while the stub is down conversion fails and the
    503 contract tests keep passing.
    """

    BINARY = b"contract-live-document"

    def __init__(self, base_url: str | None = None) -> None:
        outer = self
        self.requests: list[dict] = []
        self._lock = threading.Lock()
        payload = json.dumps(
            {
                "description_binary": base64.b64encode(self.BINARY).decode("ascii"),
                "description_html": "<p></p>",
                "description_json": {},
            }
        ).encode()

        class Handler(BaseHTTPRequestHandler):
            def _answer(self) -> None:
                length = int(self.headers.get("Content-Length") or 0)
                raw = self.rfile.read(length) if length else b""
                try:
                    parsed = json.loads(raw) if raw else None
                except ValueError:
                    parsed = None
                with outer._lock:
                    outer.requests.append(
                        {
                            "method": self.command,
                            "path": urlsplit(self.path).path,
                            "headers": dict(self.headers),
                            "body": raw,
                            "json": parsed,
                        }
                    )
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            do_GET = _answer
            do_POST = _answer
            do_PUT = _answer
            do_PATCH = _answer
            do_DELETE = _answer

            def log_message(self, *args: object) -> None:  # keep test output clean
                pass

        parts = urlsplit(base_url or config.LIVE_DOC_STUB_BASE)
        self._server = ThreadingHTTPServer((parts.hostname, parts.port), Handler)
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)

    @property
    def url(self) -> str:
        host, port = self._server.server_address
        return f"http://{host}:{port}"

    def snapshot(self) -> list[dict]:
        with self._lock:
            return list(self.requests)

    def __enter__(self) -> "LiveDocStub":
        self._thread.start()
        return self

    def __exit__(self, *args: object) -> None:
        self._server.shutdown()
        self._thread.join(timeout=5)
        self._server.server_close()


class HostileLengthSink:
    """Raw-socket sink answering a huge Content-Length, then closing (PIDASHCONV-1273).

    ``BaseHTTPRequestHandler`` cannot emit a malformed framing (it always
    pairs Content-Length with a body), so this sink speaks bare TCP: it
    reads one request's headers, replies with the configured status line
    plus ``Content-Length: 1000000000000`` and no body, and closes.
    A bounded client reports a transport error (EOF before the promised
    bytes); a client that preallocates from the header aborts instead.

    Ephemeral port, context-manager scoped (one hostile URL per test);
    the accept loop serves connections until closed so a retried
    delivery hits the same hostile framing.
    """

    def __init__(self, length: int = 1_000_000_000_000, status: str = "200 OK"):
        self._length = length
        self._status = status
        self.requests: list[bytes] = []
        self._lock = threading.Lock()
        self._server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._server.bind(("127.0.0.1", 0))
        self._server.listen(16)
        self._server.settimeout(0.5)
        self.host, self.port = self._server.getsockname()[:2]
        self._stopped = threading.Event()
        self._thread = threading.Thread(target=self._serve, daemon=True)
        self._thread.start()

    @property
    def url(self) -> str:
        return f"http://{self.host}:{self.port}"

    def _serve(self) -> None:
        while not self._stopped.is_set():
            try:
                conn, _ = self._server.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            with conn:
                try:
                    conn.settimeout(5)
                    raw = b""
                    while b"\r\n\r\n" not in raw:
                        chunk = conn.recv(65536)
                        if not chunk:
                            break
                        raw += chunk
                    with self._lock:
                        self.requests.append(raw)
                    conn.sendall(
                        f"HTTP/1.1 {self._status}\r\n"
                        f"Content-Length: {self._length}\r\n"
                        "Connection: close\r\n\r\n".encode("ascii")
                    )
                    conn.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass

    def connection_count(self) -> int:
        with self._lock:
            return len(self.requests)

    def wait_for_count(self, n: int, what: str = "hostile deliveries") -> int:
        return wait_for_condition(
            lambda: self.connection_count() if self.connection_count() >= n else None,
            what=what,
        )

    def close(self) -> None:
        self._stopped.set()
        try:
            self._server.close()
        except OSError:
            pass
        self._thread.join(timeout=10)

    def __enter__(self) -> "HostileLengthSink":
        return self

    def __exit__(self, *exc) -> None:
        self.close()


# --- PIDASHCONV-1271: forward-proxy stub for the proxy-env suites ---
def make_test_ca(directory, *, sans):
    """Mint a throwaway CA + server cert for :class:`ProxyStub` tunnels.

    Returns ``(ca_pem, cert_pem, key_pem)`` as Paths under ``directory``.
    ``sans`` are the DNS names the leaf cert is valid for (the CONNECT
    targets the test drives). The client under test must trust the CA
    (``requests`` via ``REQUESTS_CA_BUNDLE``); rustls-webpki clients
    cannot, so they only ever reach the CONNECT record, never the
    inner response.
    """
    from cryptography import x509
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import rsa
    from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID

    directory = Path(directory)
    now = datetime.datetime.now(datetime.timezone.utc)
    ca_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    ca_name = x509.Name(
        [x509.NameAttribute(NameOID.COMMON_NAME, "pidash-contract-test-ca")]
    )
    ca_cert = (
        x509.CertificateBuilder()
        .subject_name(ca_name)
        .issuer_name(ca_name)
        .public_key(ca_key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - datetime.timedelta(hours=1))
        .not_valid_after(now + datetime.timedelta(days=7))
        .add_extension(
            x509.BasicConstraints(ca=True, path_length=0), critical=True
        )
        .add_extension(
            x509.KeyUsage(
                digital_signature=True,
                content_commitment=False,
                key_encipherment=False,
                data_encipherment=False,
                key_agreement=False,
                key_cert_sign=True,
                crl_sign=True,
                encipher_only=False,
                decipher_only=False,
            ),
            critical=True,
        )
        .sign(ca_key, hashes.SHA256())
    )
    leaf_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    leaf_cert = (
        x509.CertificateBuilder()
        .subject_name(
            x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, sans[0])])
        )
        .issuer_name(ca_name)
        .public_key(leaf_key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - datetime.timedelta(hours=1))
        .not_valid_after(now + datetime.timedelta(days=7))
        .add_extension(
            x509.BasicConstraints(ca=False, path_length=None), critical=True
        )
        .add_extension(
            x509.SubjectAlternativeName([x509.DNSName(name) for name in sans]),
            critical=False,
        )
        .add_extension(
            x509.KeyUsage(
                digital_signature=True,
                content_commitment=False,
                key_encipherment=True,
                data_encipherment=False,
                key_agreement=False,
                key_cert_sign=False,
                crl_sign=False,
                encipher_only=False,
                decipher_only=False,
            ),
            critical=True,
        )
        .add_extension(
            x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH]),
            critical=False,
        )
        .sign(ca_key, hashes.SHA256())
    )
    ca_pem = directory / "proxy-ca.pem"
    cert_pem = directory / "proxy-leaf.pem"
    key_pem = directory / "proxy-leaf.key"
    ca_pem.write_bytes(ca_cert.public_bytes(serialization.Encoding.PEM))
    cert_pem.write_bytes(leaf_cert.public_bytes(serialization.Encoding.PEM))
    key_pem.write_bytes(
        leaf_key.private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.TraditionalOpenSSL,
            serialization.NoEncryption(),
        )
    )
    return ca_pem, cert_pem, key_pem


class ProxyStub:
    """Minimal forward proxy: plain absolute-URI hits plus CONNECT tunnels.

    First use: PIDASHCONV-1271, proving ``register_instance``'s release
    probe honors the proxy environment. Plain ``http`` targets arrive as
    absolute-URI requests and are answered directly; ``https`` targets
    arrive as ``CONNECT host:port``, get ``200 Connection established``,
    then speak TLS inside the tunnel with the caller-supplied test cert
    (``tls_cert``/``tls_key`` PEM paths; the client must trust its CA).
    Every hit records ``{method, path, headers, body, tunneled}`` in
    :attr:`requests` (``tunneled`` marks the inner request read from an
    established tunnel). Every answer is 200 with the fixed ``body``.
    """

    def __init__(
        self, *, body: bytes = b"", content_type: str = "application/json",
        tls_cert=None, tls_key=None,
    ):
        self._body = body
        self._content_type = content_type
        self._tls_cert = tls_cert
        self._tls_key = tls_key
        self.requests: list[dict] = []
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind(("127.0.0.1", 0))
        self._sock.listen(16)
        self._sock.settimeout(0.5)
        self._thread = threading.Thread(target=self._serve, daemon=True)

    @property
    def url(self) -> str:
        host, port = self._sock.getsockname()
        return f"http://{host}:{port}"

    def __enter__(self) -> "ProxyStub":
        self._thread.start()
        return self

    def __exit__(self, *args: object) -> None:
        self._stop.set()
        self._thread.join(timeout=10)
        self._sock.close()

    def _record(self, entry: dict) -> None:
        with self._lock:
            self.requests.append(entry)

    def _response(self) -> bytes:
        head = (
            "HTTP/1.1 200 OK\r\n"
            f"Content-Type: {self._content_type}\r\n"
            f"Content-Length: {len(self._body)}\r\n"
            "Connection: close\r\n\r\n"
        ).encode()
        return head + self._body

    @staticmethod
    def _read_request(rfile):
        line = rfile.readline(65536).decode("latin-1").strip()
        if not line:
            return None, {}
        headers = {}
        while True:
            raw = rfile.readline(65536).decode("latin-1")
            if raw in ("", "\r\n", "\n"):
                break
            name, _, value = raw.partition(":")
            headers[name.strip()] = value.strip()
        return line, headers

    def _serve(self) -> None:
        while not self._stop.is_set():
            try:
                conn, _ = self._sock.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            worker = threading.Thread(
                target=self._handle, args=(conn,), daemon=True
            )
            worker.start()

    def _handle(self, conn: socket.socket) -> None:
        try:
            conn.settimeout(10)
            line, headers = self._read_request(conn.makefile("rb"))
            if line is None:
                return
            parts = line.split(" ", 2)
            method = parts[0] if parts else ""
            target = parts[1] if len(parts) > 1 else ""
            if method.upper() == "CONNECT":
                self._record(
                    {
                        "method": "CONNECT",
                        "path": target,
                        "headers": headers,
                        "body": b"",
                        "tunneled": False,
                    }
                )
                conn.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
                if self._tls_cert is None:
                    return
                context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
                context.load_cert_chain(str(self._tls_cert), str(self._tls_key))
                try:
                    tls = context.wrap_socket(conn, server_side=True)
                except (ssl.SSLError, OSError):
                    return
                try:
                    tls.settimeout(10)
                    iline, iheaders = self._read_request(tls.makefile("rb"))
                    if iline is None:
                        return
                    iparts = iline.split(" ", 2)
                    self._record(
                        {
                            "method": iparts[0] if iparts else "",
                            "path": iparts[1] if len(iparts) > 1 else "",
                            "headers": iheaders,
                            "body": b"",
                            "tunneled": True,
                        }
                    )
                    tls.sendall(self._response())
                except (ssl.SSLError, OSError):
                    pass
                finally:
                    try:
                        tls.close()
                    except OSError:
                        pass
                return
            self._record(
                {
                    "method": method,
                    "path": target,
                    "headers": headers,
                    "body": b"",
                    "tunneled": False,
                }
            )
            conn.sendall(self._response())
        except (OSError, ssl.SSLError):
            pass
        finally:
            try:
                conn.close()
            except OSError:
                pass


# --- PIDASHCONV-1472: redirect stub + MITM proxy for the rename tests ---
#
# The renamed-repo contract tests need a stub that answers 301/307 with
# Location headers. The Rust worker reaches a plain-HTTP stub through
# PIDASH_GITHUB_API_BASE (RedirectStub); the Django worker cannot be
# pointed at a stub (it hardcodes https://api.github.com), so it goes
# through MitmProxy, which MITMs api.github.com with a test CA and
# serves the same routes inside the tunnel. Route tables are shared:
# values may contain "{base}", formatted with the server's own base so
# each leg follows same-host Locations.
def normalize_redirect_routes(routes: dict, base: str) -> dict:
    """Build a ``{(METHOD, path): (status, payload, headers)}`` table.

    Each route is ``(status, body)`` or ``(status, body, headers)``;
    ``body`` may be bytes, str, or a JSON-serializable value (served as
    ``application/json``); ``headers`` is a ``{name: value}`` dict whose
    values (and str bodies) format ``{base}``. Matching strips the query
    string.
    """
    normalized: dict[tuple[str, str], tuple[int, bytes, dict]] = {}
    for (method, path), route in routes.items():
        if len(route) == 3:
            status, body, headers = route
            headers = {k: v.format(base=base) for k, v in dict(headers).items()}
        else:
            status, body = route
            headers = {}
        if isinstance(body, (dict, list)):
            payload = json.dumps(body).encode()
            headers.setdefault("Content-Type", "application/json")
        elif isinstance(body, str):
            payload = body.format(base=base).encode()
            headers.setdefault("Content-Type", "application/json")
        else:
            payload = bytes(body)
            headers.setdefault("Content-Type", "application/json")
        normalized[(method, path)] = (status, payload, headers)
    return normalized


class RedirectStub:
    """HTTP stub dispatching (status, headers, body) by (method, path).

    Like :class:`RoutingStub` plus per-route response headers (Location
    for 301/307) and a fixed-port bind: the worker under test dials a
    base baked into its boot env, so the stub binds that port at run time
    (STALL_S3_PORT precedent). Unmapped paths answer 404. Every hit
    records ``{method, path, headers, body}`` in :attr:`requests` (full
    path, query string included).
    """

    def __init__(
        self,
        routes: dict,
        *,
        host: str = "127.0.0.1",
        port: int = 0,
    ) -> None:
        outer = self
        self.requests: list[dict] = []
        self._lock = threading.Lock()

        class Handler(BaseHTTPRequestHandler):
            def _answer(self) -> None:
                length = int(self.headers.get("Content-Length") or 0)
                raw = self.rfile.read(length) if length else b""
                with outer._lock:
                    outer.requests.append(
                        {
                            "method": self.command,
                            "path": self.path,
                            "headers": dict(self.headers),
                            "body": raw,
                        }
                    )
                route = outer._table.get((self.command, urlsplit(self.path).path))
                if route is None:
                    self.send_response(404)
                    self.send_header("Content-Type", "text/plain")
                    self.send_header("Content-Length", "9")
                    self.end_headers()
                    self.wfile.write(b"not found")
                    return
                status, payload, headers = route
                self.send_response(status)
                for name, value in headers.items():
                    if name.lower() != "content-length":
                        self.send_header(name, value)
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            do_GET = _answer
            do_POST = _answer
            do_PUT = _answer
            do_PATCH = _answer
            do_DELETE = _answer

            def log_message(self, *args: object) -> None:  # keep test output clean
                pass

        self._server = ThreadingHTTPServer((host, port), Handler)
        bound_host, bound_port = self._server.server_address[:2]
        self._table = normalize_redirect_routes(
            routes, f"http://{bound_host}:{bound_port}"
        )
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)

    @property
    def url(self) -> str:
        host, port = self._server.server_address[:2]
        return f"http://{host}:{port}"

    def snapshot(self) -> list[dict]:
        with self._lock:
            return list(self.requests)

    def __enter__(self) -> "RedirectStub":
        self._thread.start()
        return self

    def __exit__(self, *args: object) -> None:
        self._server.shutdown()
        self._thread.join(timeout=5)
        self._server.server_close()


def make_ca_bundle(directory, *, sans=("api.github.com",)):
    """Test-CA bundle (test CA + system certifi roots) for MITM workers.

    The worker under test validates the MITM leaf (test CA) and any
    relayed real-host certs (system roots) against one REQUESTS_CA_BUNDLE
    file, so the bundle concatenates both. Returns
    ``(bundle_pem, cert_pem, key_pem)`` as Paths under ``directory``.
    """
    import certifi

    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    ca_pem, cert_pem, key_pem = make_test_ca(directory, sans=list(sans))
    bundle_pem = directory / "mitm-ca-bundle.pem"
    bundle_pem.write_bytes(ca_pem.read_bytes() + Path(certifi.where()).read_bytes())
    return bundle_pem, cert_pem, key_pem


class MitmProxy:
    """Forward proxy that MITMs one host and blind-relays everything else.

    ``CONNECT <mitm_host>:443`` gets ``200 Connection established``,
    then speaks TLS inside the tunnel with the caller-supplied test cert
    and serves :func:`normalize_redirect_routes` ``routes`` (base
    ``https://<mitm_host>``, so the client follows Locations back through
    the proxy; unmapped inner paths answer 404). Any other CONNECT target
    is blindly relayed byte-for-byte (sentry/telemetry-style traffic the
    test does not stub keeps working). Plain absolute-URI requests
    answer 400 (https clients always CONNECT — anything else is a loud
    failure). Records ``{method, path, headers, body, tunneled,
    relayed}`` in :attr:`requests`.

    First use: PIDASHCONV-1472, where the Django worker (which hardcodes
    https://api.github.com) reaches the redirect stub through this proxy
    while the Rust worker uses a plain RedirectStub via
    PIDASH_GITHUB_API_BASE.
    """

    def __init__(
        self,
        routes: dict,
        *,
        mitm_host: str = "api.github.com",
        tls_cert=None,
        tls_key=None,
        host: str = "127.0.0.1",
        port: int = 0,
        relay_timeout: float = 120.0,
    ) -> None:
        self._table = normalize_redirect_routes(routes, f"https://{mitm_host}")
        self._mitm_host = mitm_host
        self._tls_cert = tls_cert
        self._tls_key = tls_key
        self._relay_timeout = relay_timeout
        self.requests: list[dict] = []
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind((host, port))
        self._sock.listen(32)
        self._sock.settimeout(0.5)
        self._thread = threading.Thread(target=self._serve, daemon=True)

    @property
    def url(self) -> str:
        host, port = self._sock.getsockname()
        return f"http://{host}:{port}"

    def snapshot(self) -> list[dict]:
        with self._lock:
            return list(self.requests)

    def saw_connect(self, host: str) -> bool:
        """True once any CONNECT to ``host`` arrived (loop-membership gate)."""
        with self._lock:
            return any(
                entry["method"] == "CONNECT"
                and entry["path"].split(":")[0].lower() == host.lower()
                for entry in self.requests
            )

    def __enter__(self) -> "MitmProxy":
        self._thread.start()
        return self

    def __exit__(self, *args: object) -> None:
        self._stop.set()
        self._thread.join(timeout=10)
        self._sock.close()

    def _record(self, entry: dict) -> None:
        with self._lock:
            self.requests.append(entry)

    @staticmethod
    def _read_request(rfile):
        line = rfile.readline(65536).decode("latin-1").strip()
        if not line:
            return None, {}
        headers = {}
        while True:
            raw = rfile.readline(65536).decode("latin-1")
            if raw in ("", "\r\n", "\n"):
                break
            name, _, value = raw.partition(":")
            headers[name.strip()] = value.strip()
        return line, headers

    def _serve(self) -> None:
        while not self._stop.is_set():
            try:
                conn, _ = self._sock.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            worker = threading.Thread(target=self._handle, args=(conn,), daemon=True)
            worker.start()

    def _handle(self, conn: socket.socket) -> None:
        try:
            conn.settimeout(10)
            line, headers = self._read_request(conn.makefile("rb"))
            if line is None:
                return
            parts = line.split(" ", 2)
            method = parts[0] if parts else ""
            target = parts[1] if len(parts) > 1 else ""
            if method.upper() != "CONNECT":
                self._record(
                    {
                        "method": method,
                        "path": target,
                        "headers": headers,
                        "body": b"",
                        "tunneled": False,
                        "relayed": False,
                    }
                )
                conn.sendall(
                    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 11\r\n"
                    b"Connection: close\r\n\r\nbad request"
                )
                return
            host = target.split(":")[0].lower()
            if host == self._mitm_host.lower():
                self._record(
                    {
                        "method": "CONNECT",
                        "path": target,
                        "headers": headers,
                        "body": b"",
                        "tunneled": False,
                        "relayed": False,
                    }
                )
                self._serve_tunnel(conn)
            else:
                self._record(
                    {
                        "method": "CONNECT",
                        "path": target,
                        "headers": headers,
                        "body": b"",
                        "tunneled": False,
                        "relayed": True,
                    }
                )
                self._relay(conn, target)
        except (OSError, ssl.SSLError):
            pass
        finally:
            try:
                conn.close()
            except OSError:
                pass

    def _serve_tunnel(self, conn: socket.socket) -> None:
        conn.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
        if self._tls_cert is None:
            return
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(str(self._tls_cert), str(self._tls_key))
        try:
            tls = context.wrap_socket(conn, server_side=True)
        except (ssl.SSLError, OSError):
            return
        try:
            tls.settimeout(10)
            rfile = tls.makefile("rb")
            iline, iheaders = self._read_request(rfile)
            if iline is None:
                return
            iparts = iline.split(" ", 2)
            imethod = iparts[0] if iparts else ""
            ipath = iparts[1] if len(iparts) > 1 else ""
            length = 0
            for name, value in iheaders.items():
                if name.lower() == "content-length":
                    try:
                        length = int(value)
                    except ValueError:
                        length = 0
            body = rfile.read(length) if length else b""
            self._record(
                {
                    "method": imethod,
                    "path": ipath,
                    "headers": iheaders,
                    "body": body,
                    "tunneled": True,
                    "relayed": False,
                }
            )
            route = self._table.get((imethod, urlsplit(ipath).path))
            if route is None:
                head = (
                    "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\n"
                    "Content-Length: 9\r\nConnection: close\r\n\r\nnot found"
                ).encode()
                tls.sendall(head)
                return
            status, payload, headers = route
            reason = {
                200: "OK",
                201: "Created",
                301: "Moved Permanently",
                307: "Temporary Redirect",
            }.get(status, "Status")
            head = [f"HTTP/1.1 {status} {reason}"]
            for name, value in headers.items():
                if name.lower() != "content-length":
                    head.append(f"{name}: {value}")
            head.append(f"Content-Length: {len(payload)}")
            head.append("Connection: close")
            tls.sendall(("\r\n".join(head) + "\r\n\r\n").encode() + payload)
        except (ssl.SSLError, OSError):
            pass
        finally:
            try:
                tls.close()
            except OSError:
                pass

    def _relay(self, client: socket.socket, target: str) -> None:
        host, _, port = target.partition(":")
        try:
            upstream = socket.create_connection((host, int(port or 443)), timeout=10)
        except (OSError, ValueError):
            try:
                client.sendall(
                    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 11\r\n"
                    b"Connection: close\r\n\r\nbad gateway"
                )
            except OSError:
                pass
            return
        try:
            client.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
            for sock in (client, upstream):
                sock.setblocking(False)
            deadline = time.monotonic() + self._relay_timeout
            while time.monotonic() < deadline:
                try:
                    ready, _, _ = select.select([client, upstream], [], [], 5)
                except (OSError, ValueError):
                    return
                if not ready:
                    continue
                for sock in ready:
                    other = upstream if sock is client else client
                    try:
                        chunk = sock.recv(65536)
                    except OSError:
                        return
                    if not chunk:
                        return
                    try:
                        other.sendall(chunk)
                    except OSError:
                        return
        finally:
            try:
                upstream.close()
            except OSError:
                pass
