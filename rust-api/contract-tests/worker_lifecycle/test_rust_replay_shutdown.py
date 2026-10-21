# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Worker-lifecycle replay: SIGTERM drains ``pidash-api`` worker and serve.

PIDASHCONV-1326: the Rust binary used to listen for SIGINT only, so a
container stop (SIGTERM) killed in-flight jobs and requests instead of
draining like Celery/gunicorn. These tests replay the Django oracle
(``test_shutdown_drain.py``) against spawned ``pidash-api`` processes:

- worker: SIGTERM mid-job settles the row (acked, never left
  ``running``) and exits 0 within the grace period.
- serve: SIGTERM mid-request keeps the process alive and the request
  completes after the lock releases.

Each test spawns its own processes against ``DATABASE_URL``; no other
consumer may serve ``rust_job_queue`` on that database while they run
(the queue has no per-consumer routing, so a second worker would race
for the probe job). Env contract:

- ``DATABASE_URL`` (required).
- ``PIDASH_BIN`` (default: ``<repo>/rust-api/target/debug/pidash-api``).

Local run (from ``rust-api/contract-tests``)::

    export DATABASE_URL=postgresql://...  # your own scratch database
    export SECRET_KEY=...
    cargo build -p pidash-api-bin  # from rust-api/
    pytest worker_lifecycle/test_rust_replay_shutdown.py
"""

from __future__ import annotations

import os
import threading
import time
import uuid

import httpx
import pytest

from _harness import rust_queue
from _harness.db import connect

from .lifecycle import (
    EXIT_BOUND_SECS,
    REPO_ROOT,
    LockHolder,
    Proc,
    SlowSink,
    blocked_backends,
    free_port,
    seed_hook,
    seed_org,
    spawn,
    wait_for_http,
)

M = "pi_dash.bgtasks"
BIN = os.environ.get("PIDASH_BIN", str(REPO_ROOT / "rust-api/target/debug/pidash-api"))

# Serve every prefix from Rust so the probe request never proxies to a
# Django upstream (edge.rs Prefix::ALL suffixes).
RUST_EDGE_FLAGS = [
    "WEB",
    "APP",
    "ASSISTANT",
    "LOOP",
    "PROMPTING",
    "SPACE",
    "LICENSE",
    "RUNNER_WEB",
    "API_V1",
    "RUNNER",
    "AUTH",
]


def _rust_env() -> dict:
    env = dict(os.environ)
    env["RUST_LOG"] = "info"
    for suffix in RUST_EDGE_FLAGS:
        env[f"PIDASH_RUST_{suffix}"] = "1"
    return env


@pytest.fixture()
def rust_worker(tmp_path):
    assert os.path.isfile(BIN), f"Rust binary not found at {BIN} (cargo build -p pidash-api-bin)"
    worker = spawn(
        [BIN, "worker", "--concurrency", "1"], tmp_path / "worker.log", _rust_env()
    )
    worker.wait_for_log("worker + scheduler loops started")
    yield worker
    worker.stop()


def test_worker_sigterm_drains_inflight_job(db_conn, rust_worker: Proc):
    """SIGTERM mid-job: the job settles (acked, not running), exit 0."""
    marker = f"lc-rs-{uuid.uuid4().hex}"
    with SlowSink() as sink:
        hook, workspace = seed_hook(db_conn, sink.url + "/hook", marker[-12:])
        celery_id = rust_queue.publish(
            f"{M}.webhook_task.webhook_send_task",
            kwargs={
                "webhook_id": str(hook["id"]),
                "slug": workspace["slug"],
                "event": "issue",
                "event_data": {"id": marker},
                "action": "POST",
                "current_site": "example.com",
                "activity": None,
            },
        )
        assert sink.arrived.wait(timeout=90), "worker never picked up the job"
        with connect() as conn, conn.cursor() as cur:
            cur.execute(
                "SELECT status FROM rust_job_queue WHERE celery_id = %s", (celery_id,)
            )
            row = cur.fetchone()
        assert row is not None and row[0] == "running", (
            f"job not mid-flight at SIGTERM time: {row}"
        )
        rc, elapsed = rust_worker.sigterm_and_wait()
        assert rc == 0, f"worker exited rc={rc}; see {rust_worker.log_path}"
        assert elapsed < EXIT_BOUND_SECS, f"drain took {elapsed:.1f}s, over grace"
        assert len(sink.requests) == 1
        with connect() as conn, conn.cursor() as cur:
            cur.execute(
                "SELECT status FROM rust_job_queue WHERE celery_id = %s", (celery_id,)
            )
            row = cur.fetchone()
            cur.execute(
                "SELECT count(*) FROM webhook_logs WHERE request_body LIKE %s",
                (f"%{marker}%",),
            )
            logs = cur.fetchone()[0]
        assert row is None, f"job row stranded after drain: {row}"
        assert logs == 1, "in-flight job did not settle its webhook_logs row"
    log = rust_worker.log_text()
    assert "worker shutting down" in log
    assert "worker loop stopped" in log


@pytest.fixture()
def rust_serve(tmp_path):
    assert os.path.isfile(BIN), f"Rust binary not found at {BIN} (cargo build -p pidash-api-bin)"
    port = free_port()
    server = spawn(
        [BIN, "serve", "--bind", f"127.0.0.1:{port}"],
        tmp_path / "serve.log",
        _rust_env(),
    )
    base = f"http://127.0.0.1:{port}"
    try:
        with connect() as conn:
            workspace, token = seed_org(conn, f"rs-{uuid.uuid4().hex[:8]}")
        path = f"/api/v1/workspaces/{workspace['slug']}/stickies/"
        wait_for_http(base + path)
        yield server, base, path, token
    finally:
        server.stop()


def test_serve_sigterm_completes_inflight_request(db_conn, rust_serve):
    """SIGTERM mid-request: the process drains, the POST completes, exit 0."""
    server, base, path, token = rust_serve
    result: dict = {}

    def do_post() -> None:
        try:
            r = httpx.post(
                base + path,
                json={"name": "note", "description_html": "<p>x</p>"},
                headers={"X-Api-Key": token},
                timeout=60.0,
            )
            result["status"] = r.status_code
            result["body"] = r.text
        except Exception as e:  # noqa: BLE001 - failure is the assertion input
            result["error"] = f"{type(e).__name__}: {e}"

    with LockHolder("stickies") as holder:
        thread = threading.Thread(target=do_post, daemon=True)
        thread.start()
        deadline = time.time() + 20
        while (
            time.time() < deadline
            and blocked_backends("stickies") == 0
            and thread.is_alive()
        ):
            time.sleep(0.2)
        assert thread.is_alive(), f"POST finished without blocking: {result}"
        assert blocked_backends("stickies") >= 1, "POST never blocked on the lock"

        def during_drain() -> None:
            holder.release()
            thread.join(timeout=30)
            assert result.get("status") == 201, f"in-flight POST lost: {result}"

        rc, elapsed = server.sigterm_and_wait(during_drain=during_drain)
        assert rc == 0, f"serve exited rc={rc}; see {server.log_path}"
        assert elapsed < EXIT_BOUND_SECS, f"drain took {elapsed:.1f}s, over grace"
