# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Dedicated Django oracle worker for the export tasks (PIDASHCONV-1106).

The stock contract worker never imports ``pi_dash.bgtasks.export_task``
or ``pi_dash.bgtasks.analytic_plot_export``, so it rejects both tasks as
unregistered (PDASHOSS01-292). The export oracle therefore boots its own
Django worker with those modules force-included::

    celery -A pi_dash worker -E --pool=solo \
        -I pi_dash.bgtasks.export_task,pi_dash.bgtasks.analytic_plot_export

This is harness-only: ``apps/api`` is untouched (the registration gap is
deliberately NOT fixed in Django). The exports suites run in CI jobs
without the shared worker, so this worker is the only consumer and no
queue routing is needed.

Env contract (same as the shared worker): ``CELERY_BROKER_URL`` (redis
in CI), the ``POSTGRES_*`` database settings, ``EMAIL_HOST``/``EMAIL_PORT``
pointing at the pytest SMTP sink, and the ``AWS_*``/``USE_MINIO`` S3
settings pointing at the S3 fixture. ``DJANGO_SETTINGS_MODULE`` must be
set by the caller (CI sets ``pi_dash.settings.local``).
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

from . import broker_probe, config

# Modules the oracle worker force-includes (the `-I` list). The task
# names served from them:
# - pi_dash.bgtasks.export_task.issue_export_task
# - pi_dash.bgtasks.analytic_plot_export.analytic_export_task
EXPORT_TASK_MODULES = (
    "pi_dash.bgtasks.export_task",
    "pi_dash.bgtasks.analytic_plot_export",
)

EXPORT_TASKS = {
    "pi_dash.bgtasks.export_task.issue_export_task",
    "pi_dash.bgtasks.analytic_plot_export.analytic_export_task",
}


def repo_root() -> Path:
    """Repository root (``apps/api`` lives directly under it)."""
    return Path(__file__).resolve().parents[3]


def worker_command() -> list[str]:
    """The oracle worker argv (without log redirect)."""
    celery = shutil.which("celery")
    if celery is not None:
        argv = [celery]
    else:  # pragma: no cover - CI always has the console script
        argv = [sys.executable, "-m", "celery"]
    return argv + [
        "-A",
        "pi_dash",
        "worker",
        "-E",
        "--pool=solo",
        "-I",
        ",".join(EXPORT_TASK_MODULES),
        "--loglevel=info",
    ]


class ExportWorker:
    """A booted oracle worker; ``stop()`` terminates it (SIGTERM, reap)."""

    def __init__(self, proc: subprocess.Popen, log_path: Path):
        self.proc = proc
        self.log_path = log_path

    @property
    def pid(self) -> int:
        return self.proc.pid

    def stop(self) -> None:
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=30)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(timeout=30)


def spawn_export_worker(*, log_path: str | os.PathLike | None = None) -> ExportWorker:
    """Boot the oracle worker and wait until it registers the tasks.

    Raises if the worker exits early (its log path is in the message) or
    the tasks never register. The caller owns ``stop()`` (a session
    fixture does it at teardown).
    """
    log = Path(log_path) if log_path is not None else Path("/tmp/export-oracle-worker.log")
    apps_api = repo_root() / "apps" / "api"
    assert apps_api.is_dir(), f"apps/api not found at {apps_api}"
    assert config.CELERY_BROKER_URL, "CELERY_BROKER_URL must be set for the oracle worker"
    log_fd = open(log, "w")  # noqa: PTH123 - deliberate CI log path
    try:
        proc = subprocess.Popen(
            worker_command(),
            cwd=str(apps_api),
            stdout=log_fd,
            stderr=subprocess.STDOUT,
            env=os.environ.copy(),
        )
    finally:
        log_fd.close()
    worker = ExportWorker(proc, log)
    try:
        broker_probe.wait_for_registration(EXPORT_TASKS, what="export oracle worker")
    except Exception:
        exited = proc.poll() is not None
        worker.stop()
        if exited:
            raise RuntimeError(f"oracle worker exited early; see {log}") from None
        raise RuntimeError(f"oracle worker never registered; see {log}") from None
    return worker
