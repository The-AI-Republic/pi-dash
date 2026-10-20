"""Local stub sinks: capture outbound side effects without touching Django."""

from __future__ import annotations

import json
import socket
import threading
from email import message_from_bytes
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
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
    value (dict/list, served as ``application/json``). Unmapped paths
    answer 404. Every hit records ``{method, path, headers, body}`` in
    :attr:`requests` (full path, query string included).
    """

    def __init__(self, routes: dict[tuple[str, str], tuple[int, object]]) -> None:
        outer = self
        self.requests: list[dict] = []
        normalized: dict[tuple[str, str], tuple[int, bytes, str]] = {}
        for (method, path), (status, body) in routes.items():
            if isinstance(body, (dict, list)):
                payload, content_type = json.dumps(body).encode(), "application/json"
            elif isinstance(body, str):
                payload, content_type = body.encode(), "application/json"
            else:
                payload, content_type = bytes(body), "application/json"
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
