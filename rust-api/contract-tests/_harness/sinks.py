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
