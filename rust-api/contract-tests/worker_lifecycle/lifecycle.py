# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared process-lifecycle probes (PIDASHCONV-1326).

Both the Django oracle (``test_shutdown_drain.py``) and the Rust replay
(``test_rust_replay_shutdown.py``) spawn their own worker and server
processes — the shared CI servers cannot be SIGTERMed — and drive the
same two scenarios:

- worker: a ``webhook_send_task`` mid-POST to a slow sink gets SIGTERM;
  the job must settle and the process must exit 0 within the grace period.
- serve: a sticky POST blocked on a held table lock gets SIGTERM; the
  process must stay alive, and the request must complete after unlock.

Timing budget (SIGTERM-to-exit must stay inside the 30 s grace both
backends honour): the sink sleeps 8 s, well under the 30 s webhook
request timeout each side configures, and the exit bound is 25 s.
"""

from __future__ import annotations

import os
import signal
import socket
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import psycopg

from _harness import seed as seed_helpers
from _harness.db import connect, get_database_url

# The sink answers 200 after this delay. Under the 30 s webhook timeout
# on both backends (Django ``requests.post(..., timeout=30)``,
# ``webhook_send.rs``), over the ~1 s SIGTERM-to-death of the bug.
SLOW_SECS = 8.0
# SIGTERM-to-exit must fit the 30 s grace (Rust main.rs, gunicorn
# graceful-timeout): the drain takes SLOW_SECS, the bound leaves margin.
EXIT_BOUND_SECS = 25.0
# A draining process is still alive 1 s after SIGTERM; the bug kills in ~0.1 s.
DRAIN_ALIVE_SECS = 1.0

REPO_ROOT = Path(__file__).resolve().parents[3]


class SlowSink:
    """HTTP stub that records arrival, sleeps, then answers 200.

    ``arrived`` fires when the first request body is read, so the test
    knows the worker task is mid-flight deterministically on both
    backends instead of guessing with a sleep.
    """

    def __init__(self, slow_secs: float = SLOW_SECS) -> None:
        self.slow_secs = slow_secs
        self.arrived = threading.Event()
        self.requests: list[dict] = []
        outer = self

        class Handler(BaseHTTPRequestHandler):
            def _answer(self) -> None:
                length = int(self.headers.get("Content-Length") or 0)
                body = self.rfile.read(length) if length else b""
                outer.requests.append(
                    {
                        "method": self.command,
                        "path": self.path,
                        "headers": dict(self.headers.items()),
                        "body": body.decode("utf8", "replace"),
                    }
                )
                outer.arrived.set()
                time.sleep(outer.slow_secs)
                payload = b'{"ok": true}'
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                try:
                    self.wfile.write(payload)
                except (BrokenPipeError, ConnectionResetError):
                    pass

            do_POST = _answer
            do_PUT = _answer
            do_PATCH = _answer
            do_GET = _answer

            def log_message(self, *args: object) -> None:  # quiet pytest output
                pass

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)

    @property
    def url(self) -> str:
        host, port = self._server.server_address
        return f"http://{host}:{port}"

    def __enter__(self) -> "SlowSink":
        self._thread.start()
        return self

    def __exit__(self, *args: object) -> None:
        self._server.shutdown()
        self._thread.join(timeout=10)
        self._server.server_close()


class Proc:
    """A spawned worker/server with log capture and guaranteed cleanup."""

    def __init__(self, proc: subprocess.Popen, log_path: Path) -> None:
        self.proc = proc
        self.log_path = log_path

    @property
    def pid(self) -> int:
        return self.proc.pid

    def wait_for_log(self, text: str, timeout: float = 120.0) -> None:
        """Block until the process logs ``text`` (boot readiness)."""
        deadline = time.time() + timeout
        while time.time() < deadline:
            if self.proc.poll() is not None:
                raise RuntimeError(
                    f"process exited rc={self.proc.poll()} before logging "
                    f"{text!r}; see {self.log_path}"
                )
            if text in self.log_path.read_text():
                return
            time.sleep(0.2)
        raise RuntimeError(f"timed out waiting for {text!r}; see {self.log_path}")

    def sigterm_and_wait(
        self,
        timeout: float = EXIT_BOUND_SECS + 15.0,
        during_drain=None,
    ) -> tuple[int, float]:
        """SIGTERM, assert draining (alive after 1 s), wait for exit.

        ``during_drain``, when given, runs after the alive check while
        the process drains (e.g. the serve tests release their table
        lock there). Returns ``(returncode, elapsed)``. Raises if the
        process is already dead 1 s after SIGTERM (the PIDASHCONV-1326
        bug: killed at once instead of draining) or never exits.
        """
        assert self.proc.poll() is None, "process already exited before SIGTERM"
        start = time.time()
        self.proc.send_signal(signal.SIGTERM)
        time.sleep(DRAIN_ALIVE_SECS)
        assert self.proc.poll() is None, (
            f"process died within {DRAIN_ALIVE_SECS}s of SIGTERM "
            f"(rc={self.proc.poll()}) instead of draining; see {self.log_path}"
        )
        if during_drain is not None:
            during_drain()
        try:
            rc = self.proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self.stop()
            raise RuntimeError(
                f"process never exited after SIGTERM; see {self.log_path}"
            ) from None
        return rc, time.time() - start

    def stop(self) -> None:
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(timeout=15)

    def log_text(self) -> str:
        return self.log_path.read_text() if self.log_path.exists() else ""


def spawn(argv: list[str], log_path: Path, env: dict, cwd: str | None = None) -> Proc:
    with open(log_path, "w") as log_fd:
        proc = subprocess.Popen(
            argv, stdout=log_fd, stderr=subprocess.STDOUT, env=env, cwd=cwd
        )
    return Proc(proc, log_path)


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def wait_for_http(url: str, timeout: float = 120.0) -> int:
    """Poll until any HTTP status arrives; return it (boot readiness)."""
    import httpx

    deadline = time.time() + timeout
    last: Exception | None = None
    while time.time() < deadline:
        try:
            return httpx.get(url, timeout=2.0).status_code
        except Exception as e:  # noqa: BLE001 - readiness probe
            last = e
            time.sleep(0.3)
    raise RuntimeError(f"server never ready at {url}: {last}")


def seed_hook(conn, sink_url: str, tag: str) -> tuple[dict, dict]:
    """User → workspace → active webhook posting at ``sink_url``."""
    owner = seed_helpers.user(conn, f"lifecycle-{tag}-owner")
    workspace = seed_helpers.workspace(conn, f"lifecycle{tag}", owner["id"])
    hook = seed_helpers.webhook(conn, workspace["id"], sink_url, issue=True)
    return dict(hook), workspace


def seed_org(conn, tag: str) -> tuple[dict, str]:
    """User → workspace → membership → API token; return (workspace, token)."""
    admin = seed_helpers.create_user(
        conn,
        email=f"admin-lifecycle-{tag}@ct.example.com",
        username=f"admin-lifecycle-{tag}",
        password_field="x" * 60,
    )
    workspace = seed_helpers.workspace(conn, f"lifecycle{tag}", admin["id"])
    seed_helpers.add_workspace_member(
        conn,
        workspace_id=workspace["id"],
        user_id=admin["id"],
        role=seed_helpers.ADMIN,
    )
    token = seed_helpers.create_api_token(
        conn, user_id=admin["id"], workspace_id=workspace["id"]
    )
    return workspace, token


class LockHolder:
    """Holds ``LOCK TABLE <table> IN ACCESS EXCLUSIVE MODE`` until released.

    A request that touches the table blocks in the database driver with
    no fixed sleeps and no backend-specific hooks, on Django and Rust
    alike. Uses its own non-autocommit connection; closing without
    ``release()`` rolls back.
    """

    def __init__(self, table: str) -> None:
        self.table = table
        self._conn: psycopg.Connection | None = None

    def __enter__(self) -> "LockHolder":
        assert self._conn is None
        self._conn = psycopg.connect(get_database_url())
        self._conn.execute(f"LOCK TABLE {self.table} IN ACCESS EXCLUSIVE MODE")
        return self

    def release(self) -> None:
        if self._conn is not None:
            self._conn.commit()
            self._conn.close()
            self._conn = None

    def __exit__(self, *args: object) -> None:
        if self._conn is not None:
            self._conn.rollback()
            self._conn.close()
            self._conn = None


def blocked_backends(table: str) -> int:
    """Backends currently lock-waiting on a query touching ``table``."""
    with connect() as conn, conn.cursor() as cur:
        cur.execute(
            "SELECT count(*) FROM pg_stat_activity "
            "WHERE wait_event_type = 'Lock' AND query ILIKE %s",
            (f"%{table}%",),
        )
        row = cur.fetchone()
    return row[0]


def require_env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"worker_lifecycle tests require {name} to be set")
    return value
