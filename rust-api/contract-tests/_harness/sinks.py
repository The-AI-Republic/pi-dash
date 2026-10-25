"""Local stub sinks: capture outbound side effects without touching Django."""

from __future__ import annotations

import base64
import datetime
import json
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
