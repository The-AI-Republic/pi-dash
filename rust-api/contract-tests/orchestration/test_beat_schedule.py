"""Beat-schedule firing parity: the domain tasks beat fires are the tasks
this suite publishes, and their cadences are pinned.

The schedule itself lives in apps/api/pi_dash/celery.py (Python stays in
place, so this file is readable in both the Django and the Rust runs).
Behaviorally, each scan task is published over the broker and must be
consumed without error — proving the task name, exchange and signature
the Rust worker will need to honor.
"""

import re
from pathlib import Path

from _harness import broker_redis as broker, db
from _harness.db import get_database_url

SCAN_TICKERS = "pi_dash.bgtasks.agent_ticker.scan_due_tickers"
SCAN_BINDINGS = "pi_dash.bgtasks.scheduler.scan_due_bindings"
SCAN_LOOPS = "pi_dash.bgtasks.loop.scan_due_targets"


def _celery_py() -> str:
    here = Path(__file__).resolve()
    for parent in here.parents:
        candidate = parent / "apps" / "api" / "pi_dash" / "celery.py"
        if candidate.exists():
            return candidate.read_text()
    raise AssertionError("apps/api/pi_dash/celery.py not found above contract-tests")


def test_beat_schedule_pins_domain_scans():
    """The three per-minute scans the engine depends on are all on beat with
    a one-minute cadence."""
    body = _celery_py()
    for task in (SCAN_TICKERS, SCAN_BINDINGS, SCAN_LOOPS):
        assert task in body, f"beat schedule lost {task}"
    minute_scans = re.findall(r'"task":\s*"(pi_dash\.bgtasks\.(?:agent_ticker|loop|scheduler)\.[^"]+)"\s*,\s*"schedule":\s*crontab\(minute="\*"\)', body)
    assert SCAN_TICKERS in minute_scans
    assert SCAN_BINDINGS in minute_scans
    assert SCAN_LOOPS in minute_scans


def test_scan_tasks_execute_empty_via_broker():
    """Each scan task published with no due rows: consumed cleanly, queue
    drains, database untouched."""
    tickers_before = db.fetchone(get_database_url(), "SELECT count(*) AS n FROM issue_agent_ticker")["n"]
    runs_before = db.fetchone(get_database_url(), "SELECT count(*) AS n FROM agent_run")["n"]
    broker.purge()

    for task in (SCAN_TICKERS, SCAN_BINDINGS, SCAN_LOOPS):
        broker.publish_task(task)
    db.wait_for_condition(lambda: broker.queue_depth() == 0, what="broker queue drains")

    assert db.fetchone(get_database_url(), "SELECT count(*) AS n FROM issue_agent_ticker")["n"] == tickers_before
    assert db.fetchone(get_database_url(), "SELECT count(*) AS n FROM agent_run")["n"] == runs_before
