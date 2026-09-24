"""Local stub sinks: capture outbound side effects without touching Django."""
from __future__ import annotations

import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


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
