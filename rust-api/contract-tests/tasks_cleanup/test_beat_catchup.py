# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Beat crontab catch-up parity (PIDASHCONV-1328).

Django's beat fires a crontab occurrence missed during scheduler downtime
once on recovery: ``django_celery_beat`` persists ``last_run_at`` per entry
and Celery's ``crontab.is_due`` reports due while the next occurrence after
that timestamp is in the past. The Rust scheduler used to fire only when
the current minute matched the schedule, silently dropping the missed
occurrence until the next cycle.

Run form: the Celery-oracle tests need no infrastructure (Celery ships in
``requirements.txt``); the live test needs the Rust worker running against
``DATABASE_URL`` (``pidash-api worker`` runs the scheduler loop too)::

    export DATABASE_URL=postgresql://...  # your own scratch database
    pidash-api worker &
    pytest -p no:django tasks_cleanup/test_beat_catchup.py
"""

from __future__ import annotations

import time
from datetime import datetime, timedelta, timezone
from pathlib import Path

from celery.schedules import crontab

from _harness import rust_queue
from _harness.db import execute, fetchone, get_database_url, wait_for_condition

ENTRY = "check-every-day-to-delete-hard-delete"


def _celery_py() -> str:
    here = Path(__file__).resolve()
    for parent in here.parents:
        candidate = parent / "apps" / "api" / "pi_dash" / "celery.py"
        if candidate.exists():
            return candidate.read_text()
    raise AssertionError("apps/api/pi_dash/celery.py not found above contract-tests")


def test_celery_py_uses_database_scheduler():
    """``celery.py`` persists beat state via ``DatabaseScheduler``.

    Pins the Django half the Rust schedule table mirrors: ``ModelEntry``
    passes the DB-persisted ``last_run_at`` to ``crontab.is_due``
    (django_celery_beat 2.6.0 ``schedulers.py``), and no
    ``beat_cron_starting_deadline`` suppresses the catch-up.
    """
    body = _celery_py()
    assert 'beat_scheduler = "django_celery_beat.schedulers.DatabaseScheduler"' in body
    assert "beat_cron_starting_deadline" not in body
    assert '"check-every-day-to-delete-hard-delete"' in body


def test_celery_crontab_catches_up_missed_daily_occurrence():
    """Celery oracle: a day-stale ``last_run_at`` is due exactly once."""
    sched = crontab(hour=0, minute=0)
    now = datetime.now(timezone.utc)
    stale = now.replace(hour=0, minute=0, second=0, microsecond=0) - timedelta(days=1)

    # Today's 00:00 passed after the last run: beat fires on recovery.
    due, _ = sched.is_due(stale)
    assert due is True

    # Once fired, the next evaluation is quiet until tomorrow's 00:00:
    # catch-up fires once, not once per missed occurrence.
    due_again, _ = sched.is_due(now)
    assert due_again is False


def test_rust_worker_liveness():
    """A no-op cleanup job drains, proving a Rust worker serves this DB."""
    rust_queue.publish("pi_dash.bgtasks.cleanup_task.delete_api_logs")
    rust_queue.wait_for_queue_drain(what="rust worker consumed liveness probe")


def test_rust_scheduler_catches_up_stale_daily_entry():
    """A stale ``rust_job_schedule`` row fires on the next scheduler tick.

    Seeds ``last_run_at`` = yesterday 00:00 UTC for the daily hard-delete
    entry and waits for ``fire_due`` to advance it. The advance is the
    enqueue proof: the scheduler records the fire time in the same
    transaction as the enqueue (``scheduler.rs: fire_due``), and the queue
    row itself is unobservable — the worker loop acks (deletes) it within
    the poll gap.
    """
    url = get_database_url()
    row = fetchone(url, "SELECT to_regclass('public.rust_job_schedule') AS t")
    assert row and row["t"], (
        "rust_job_schedule missing — is pidash-api worker running against DATABASE_URL?"
    )

    # The pre-fix scheduler fires inside the matching minute, so a run
    # landing on 00:00 UTC would pass even before the fix. Step past the
    # minute (bounded wait, ~1/1440 runs) to keep fail-before honest.
    now = datetime.now(timezone.utc)
    if now.hour == 0 and now.minute == 0:
        target = now.replace(minute=1, second=5, microsecond=0)
        time.sleep(max(1.0, (target - now).total_seconds()))

    stale = datetime.now(timezone.utc).replace(
        hour=0, minute=0, second=0, microsecond=0
    ) - timedelta(days=1)
    execute(
        url,
        "INSERT INTO rust_job_schedule (name, last_run_at) VALUES (%s, %s) "
        "ON CONFLICT (name) DO UPDATE SET last_run_at = EXCLUDED.last_run_at",
        (ENTRY, stale),
    )

    def _advanced():
        current = fetchone(
            url, "SELECT last_run_at FROM rust_job_schedule WHERE name = %s", (ENTRY,)
        )
        if current and current["last_run_at"] > stale:
            return current["last_run_at"]
        return None

    fired_at = wait_for_condition(
        _advanced,
        timeout=60.0,
        what=(
            f"scheduler catch-up fire for {ENTRY} "
            "(is pidash-api worker — scheduler loop — running on this DB, "
            "and does no other scheduler hold the singleton lock?)"
        ),
    )

    # Catch-up fires once: a few more ticks must not move the row again.
    time.sleep(3.0)
    rest = fetchone(
        url, "SELECT last_run_at FROM rust_job_schedule WHERE name = %s", (ENTRY,)
    )
    assert rest and rest["last_run_at"] == fired_at
