# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Worker-lifecycle oracle: SIGTERM drains the Django worker and server.

PIDASHCONV-1326: Docker/Compose/Kubernetes stop containers with SIGTERM.
Celery treats it as a warm shutdown (finish in-flight tasks, then exit)
and gunicorn finishes in-flight requests within ``graceful_timeout``.
These tests pin that behavior; the Rust replay
(``test_rust_replay_shutdown.py``) asserts the same of ``pidash-api``.

Each test spawns its own processes (a shared server cannot be SIGTERMed)
against ``DATABASE_URL``; the celery worker consumes ``CELERY_BROKER_URL``
with no other consumer on that broker DB. Env contract:

- ``DATABASE_URL`` (required), ``CELERY_BROKER_URL`` (required).
- ``DJANGO_PYTHON`` (default: this interpreter), ``DJANGO_API_DIR``
  (default: ``<repo>/apps/api``).
- ``gunicorn`` (production.txt) must be importable by ``DJANGO_PYTHON``.

Local run (from ``rust-api/contract-tests``)::

    export DATABASE_URL=postgresql://...  # your own scratch database
    export CELERY_BROKER_URL=redis://127.0.0.1:6379/<your-db>
    export DJANGO_SETTINGS_MODULE=pi_dash.settings.test
    export SECRET_KEY=...
    pytest worker_lifecycle/test_shutdown_drain.py
"""

from __future__ import annotations

import os
import sys
import threading
import time
import uuid

import httpx
import pytest

from _harness import celery_wire
from _harness.db import connect

from .lifecycle import (
    EXIT_BOUND_SECS,
    REPO_ROOT,
    LockHolder,
    Proc,
    SlowSink,
    blocked_backends,
    free_port,
    require_env,
    seed_hook,
    seed_org,
    spawn,
    wait_for_http,
)

M = "pi_dash.bgtasks"
DJANGO_PYTHON = os.environ.get("DJANGO_PYTHON", sys.executable)
DJANGO_API_DIR = os.environ.get("DJANGO_API_DIR", str(REPO_ROOT / "apps" / "api"))


def _celery_env() -> dict:
    env = dict(os.environ)
    # Broker-consumed execution only: eager flags would run .delay() calls
    # in-process for any client sharing this env, never the worker path.
    env.pop("CELERY_TASK_ALWAYS_EAGER", None)
    env.pop("CELERY_TASK_EAGER", None)
    return env


@pytest.fixture()
def celery_worker(tmp_path):
    require_env("CELERY_BROKER_URL")
    worker = spawn(
        [
            DJANGO_PYTHON,
            "-m",
            "celery",
            "-A",
            "pi_dash",
            "worker",
            "--pool=solo",
            "--concurrency=1",
            "--loglevel=info",
        ],
        tmp_path / "celery.log",
        _celery_env(),
        cwd=DJANGO_API_DIR,
    )
    worker.wait_for_log("ready.")
    yield worker
    worker.stop()


def test_worker_sigterm_drains_inflight_job(db_conn, celery_worker: Proc):
    """SIGTERM mid-task: celery warm-shuts-down, the task settles, exit 0."""
    marker = f"lc-dj-{uuid.uuid4().hex}"
    with SlowSink() as sink:
        hook, workspace = seed_hook(db_conn, sink.url + "/hook", marker[-12:])
        celery_wire.publish(
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
        rc, elapsed = celery_worker.sigterm_and_wait()
        assert rc == 0, f"celery exited rc={rc}; see {celery_worker.log_path}"
        assert elapsed < EXIT_BOUND_SECS, f"drain took {elapsed:.1f}s, over grace"
        assert len(sink.requests) == 1
        with connect() as conn, conn.cursor() as cur:
            cur.execute(
                "SELECT count(*) FROM webhook_logs WHERE request_body LIKE %s",
                (f"%{marker}%",),
            )
            logs = cur.fetchone()[0]
        assert logs == 1, "in-flight task did not settle its webhook_logs row"
    assert "Warm shutdown" in celery_worker.log_text()


@pytest.fixture()
def gunicorn(tmp_path):
    port = free_port()
    server = spawn(
        [
            DJANGO_PYTHON,
            "-m",
            "gunicorn",
            "pi_dash.asgi:application",
            "-k",
            "uvicorn.workers.UvicornWorker",
            "-w",
            "1",
            "--bind",
            f"127.0.0.1:{port}",
            "--graceful-timeout",
            "30",
            "--access-logfile",
            "-",
        ],
        tmp_path / "gunicorn.log",
        dict(os.environ),
        cwd=DJANGO_API_DIR,
    )
    base = f"http://127.0.0.1:{port}"
    try:
        with connect() as conn:
            workspace, token = seed_org(conn, f"dj-{uuid.uuid4().hex[:8]}")
        path = f"/api/v1/workspaces/{workspace['slug']}/stickies/"
        wait_for_http(base + path)
        yield server, base, path, token
    finally:
        server.stop()


def test_serve_sigterm_completes_inflight_request(db_conn, gunicorn):
    """SIGTERM mid-request: gunicorn drains, the POST completes, exit 0."""
    server, base, path, token = gunicorn
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
        assert rc == 0, f"gunicorn exited rc={rc}; see {server.log_path}"
        assert elapsed < EXIT_BOUND_SECS, f"drain took {elapsed:.1f}s, over grace"
