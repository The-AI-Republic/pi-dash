# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Black-hole TCP listener for S3 stall tests (PIDASHCONV-1291).

Binds ``127.0.0.1``, accepts connections and holds them open forever
without sending a byte — the "wedged object store" a bounded S3 client
must time out on (botocore: 60s connect / 60s read) instead of hanging.

The worker under test boots with its S3 endpoint pointed at this
listener (``AWS_S3_ENDPOINT_URL=http://127.0.0.1:<port>``); the port is
fixed before the worker boots via ``STALL_S3_PORT`` (default 4599), and
the listener itself only needs to exist while the stalled job runs —
both backends dial S3 lazily per task run.
"""

from __future__ import annotations

import contextlib
import os
import socket
import threading

DEFAULT_STALL_PORT = 4599


def stall_port() -> int:
    """The stall port the worker's S3 endpoint points at."""
    return int(os.environ.get("STALL_S3_PORT", str(DEFAULT_STALL_PORT)))


class BlackHole:
    """An accept-and-hold listener; ``hits`` counts dials so far."""

    def __init__(self, port: int):
        self._srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._srv.bind(("127.0.0.1", port))
        self._srv.listen(50)
        self._srv.settimeout(0.5)
        self.port = port
        self._held: list[socket.socket] = []
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._serve, daemon=True)

    @property
    def hits(self) -> int:
        return len(self._held)

    def _serve(self) -> None:
        while not self._stop.is_set():
            try:
                conn, _ = self._srv.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            self._held.append(conn)

    def start(self) -> "BlackHole":
        self._thread.start()
        return self

    def close(self) -> None:
        self._stop.set()
        self._thread.join(timeout=5)
        for conn in self._held:
            try:
                conn.close()
            except OSError:
                pass
        try:
            self._srv.close()
        except OSError:
            pass


@contextlib.contextmanager
def black_hole(port: int | None = None):
    """Hold a black hole open for one stalled job; yields the listener."""
    listener = BlackHole(port if port is not None else stall_port()).start()
    try:
        yield listener
    finally:
        listener.close()
