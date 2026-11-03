# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Scripted stub S3 for the retry-contract tests (PIDASHCONV-1697).

A minimal raw-socket HTTP server speaking just enough S3 to pin the
retry contract: path-style ``HEAD /<bucket>/<key>`` and ``PUT
/<bucket>/<key>`` with ``x-amz-copy-source``. Each test binds the stub
on the port the worker's S3 endpoint points at (``S3RETRY_S3_PORT``,
default 4598; both backends dial S3 lazily per task run, so the stub
only needs to exist while the instrumented job runs — the same shape
as the stall black hole in :mod:`_harness.stall`).

The script is an ordered list of actions, one per incoming request;
when the script runs out the last action repeats, so an over-eager
client stays responsive and the overage shows up in ``hits`` instead
of hanging. Actions:

- ``("respond", status, headers, body)`` — one HTTP response.
- ``("drop",)`` — read the request, then close without a byte (the
  client's connection error; botocore retries it like any transport
  failure).

Every response carries ``Connection: close`` so neither backend pools
a stale socket across stub restarts. Success bodies are real S3 XML:
botocore's ``check_for_200_error`` (needs-retry.s3.CopyObject) treats
a 200 whose body is not valid non-``<Error>`` XML as a 500, so a bare
or empty success body would be retried by Django.
"""

from __future__ import annotations

import contextlib
import os
import socket
import threading
from dataclasses import dataclass, field

DEFAULT_STUB_PORT = 4598

REASONS = {
    200: "OK",
    400: "Bad Request",
    403: "Forbidden",
    404: "Not Found",
    429: "Too Many Requests",
    500: "Internal Server Error",
    502: "Bad Gateway",
    503: "Service Unavailable",
    504: "Gateway Timeout",
    509: "Bandwidth Limit Exceeded",
}

COPY_OK_XML = (
    '<?xml version="1.0" encoding="UTF-8"?>'
    '<CopyObjectResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">'
    "<LastModified>2026-11-01T12:00:00.000Z</LastModified>"
    '<ETag>"d41d8cd98f00b204e9800998ecf8427e"</ETag>'
    "</CopyObjectResult>"
).encode()

HEAD_ETAG = '"5d41402abc4b2a76b9719d911017c592"'
# Consistent IMF-fixdate (Nov 1 2026 is a Sunday): botocore ignores a
# mismatched weekday, but chrono's RFC 2822 parser rejects it.
HEAD_LAST_MODIFIED = "Sun, 01 Nov 2026 12:00:00 GMT"


def stub_port() -> int:
    """The stub port the worker's S3 endpoint points at."""
    return int(os.environ.get("S3RETRY_S3_PORT", str(DEFAULT_STUB_PORT)))


def s3_error_xml(code: str, message: str) -> bytes:
    """One S3 ``<Error>`` body with the given ``<Code>``."""
    return (
        '<?xml version="1.0" encoding="UTF-8"?>'
        f"<Error><Code>{code}</Code><Message>{message}</Message>"
        "<Resource>/stub</Resource><RequestId>stub-1</RequestId></Error>"
    ).encode()


def head_ok_headers(length: int, content_type: str = "application/octet-stream") -> dict:
    """HEAD success headers; botocore parses these into the head dict."""
    return {
        "Content-Type": content_type,
        "Content-Length": str(length),
        "ETag": HEAD_ETAG,
        "Last-Modified": HEAD_LAST_MODIFIED,
        "x-amz-request-id": "stub-1",
    }


@dataclass
class Hit:
    """One request the stub served: method, raw path, headers, body."""

    method: str
    path: str
    headers: dict
    body: bytes


@dataclass
class StubS3:
    """A scripted stub S3; ``hits`` records every request served."""

    port: int
    script: list
    hits: list = field(default_factory=list, init=False)
    stored: dict = field(default_factory=dict, init=False)

    def __post_init__(self) -> None:
        self._srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._srv.bind(("127.0.0.1", self.port))
        self._srv.listen(50)
        self._srv.settimeout(0.5)
        self._at = 0
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._serve, daemon=True)

    def start(self) -> "StubS3":
        self._thread.start()
        return self

    def close(self) -> None:
        self._stop.set()
        self._thread.join(timeout=5)
        try:
            self._srv.close()
        except OSError:
            pass

    def attempts(self, method: str | None = None) -> int:
        """Hits so far, optionally filtered by method."""
        with self._lock:
            if method is None:
                return len(self.hits)
            return sum(1 for hit in self.hits if hit.method == method)

    def stored_paths(self) -> dict:
        """Destination path -> copy-source for every 200 PUT-copy so far."""
        with self._lock:
            return dict(self.stored)

    def _next_action(self):
        with self._lock:
            action = self.script[min(self._at, len(self.script) - 1)]
            self._at += 1
            return action

    def _serve(self) -> None:
        while not self._stop.is_set():
            try:
                conn, _ = self._srv.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            try:
                self._handle(conn)
            except OSError:
                pass
            finally:
                try:
                    conn.close()
                except OSError:
                    pass

    def _handle(self, conn: socket.socket) -> None:
        conn.settimeout(10)
        data = b""
        while b"\r\n\r\n" not in data:
            chunk = conn.recv(65536)
            if not chunk:
                return
            data += chunk
        head, _, rest = data.partition(b"\r\n\r\n")
        lines = head.decode("latin-1").split("\r\n")
        method, path, _ = lines[0].split(" ", 2)
        headers: dict[str, str] = {}
        for line in lines[1:]:
            name, _, value = line.partition(":")
            headers[name.strip().lower()] = value.strip()
        length = int(headers.get("content-length", "0") or "0")
        body = rest[:length]
        while len(body) < length:
            chunk = conn.recv(65536)
            if not chunk:
                break
            body += chunk
        hit = Hit(method=method, path=path, headers=headers, body=body)
        with self._lock:
            self.hits.append(hit)
        if headers.get("expect", "").lower() == "100-continue":
            conn.sendall(b"HTTP/1.1 100 Continue\r\n\r\n")
        action = self._next_action()
        if action[0] == "drop":
            return
        _, status, extra, resp_body = action
        if method == "PUT" and 200 <= status < 300 and "x-amz-copy-source" in headers:
            with self._lock:
                self.stored[path] = headers["x-amz-copy-source"]
        reason = REASONS.get(status, "Error")
        out = [f"HTTP/1.1 {status} {reason}\r\n"]
        if method != "HEAD":
            out.append(f"Content-Length: {len(resp_body)}\r\n")
        for name, value in extra.items():
            out.append(f"{name}: {value}\r\n")
        out.append("Connection: close\r\n\r\n")
        conn.sendall("".join(out).encode("latin-1"))
        if method != "HEAD":
            conn.sendall(resp_body)


@contextlib.contextmanager
def stub_s3(script: list, port: int | None = None):
    """Serve one scripted stub S3; yields the stub (hits/stored)."""
    stub = StubS3(port if port is not None else stub_port(), script).start()
    try:
        yield stub
    finally:
        stub.close()
