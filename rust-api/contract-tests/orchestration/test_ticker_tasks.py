"""Ticker tasks over the wire: scan fan-out, fire_tick claim/skip/rollback,
ETA gating, and single-execution under redelivery.

Task names pinned here are the Celery contract both backends honor:
- pi_dash.bgtasks.agent_ticker.scan_due_tickers (beat: every minute)
- pi_dash.bgtasks.agent_ticker.fire_tick (one per due ticker, arg: ticker PK)
"""

import uuid

from _harness import broker_redis as broker, db, world
from _harness.db import get_database_url

SCAN = "pi_dash.bgtasks.agent_ticker.scan_due_tickers"
FIRE = "pi_dash.bgtasks.agent_ticker.fire_tick"


def _ticker(issue):
    return db.fetchone(get_database_url(), "SELECT * FROM issue_agent_ticker WHERE issue_id=%s", (issue,))


def _due(issue):
    db.execute(get_database_url(), "UPDATE issue_agent_ticker SET next_run_at=now() - interval '5 minutes' WHERE issue_id=%s",
               (issue,))


def _drain():
    db.wait_for_condition(lambda: broker.queue_depth() == 0, what="broker queue drains")


def _claimable(w, alias):
    """Issue whose fire_tick reaches the claim but whose dispatch fails
    (a prior run exists, yet no default pod): last_tick_at moves as the
    observability mark while the budget rolls back."""
    issue = world.make_issue(w, alias, "In Progress")
    world.set_ticker(issue)
    _due(issue)
    pod = world.make_pod(w.workspace_id, w.project_id)
    world.make_run(issue, w.workspace_id, w.user_id, pod, status="completed")
    return issue


def _runs(issue):
    return db.fetchone(get_database_url(), "SELECT count(*) AS n FROM agent_run WHERE work_item_id=%s", (issue,))["n"]


def test_scan_fans_out_one_fire_per_due_ticker(api_client):
    w, _ = api_client
    a = _claimable(w, "scan-a")
    b = _claimable(w, "scan-b")
    broker.purge()

    broker.publish_task(SCAN)
    # Both fan-outs execute: each leaves its last_tick_at observability mark
    # (budget itself rolls back — asserted below).
    db.wait_for_condition(lambda: _ticker(a)["last_tick_at"] is not None, what="fire_tick(a) executed")
    db.wait_for_condition(lambda: _ticker(b)["last_tick_at"] is not None, what="fire_tick(b) executed")
    _drain()

    for issue in (a, b):
        tick = _ticker(issue)
        assert tick["used"] == 0  # claim rolled back: dispatch failed
        assert _runs(issue) == 1  # only the seeded prior run


def test_fire_tick_skips_without_prior_run(api_client):
    """No previous run on the issue: pre-claim skip, ticker row identical,
    no run created."""
    w, _ = api_client
    issue = world.make_issue(w, "noprior", "In Progress")
    world.set_ticker(issue)
    _due(issue)
    ticker_id = str(_ticker(issue)["id"])
    before = _ticker(issue)
    broker.purge()

    broker.publish_task(FIRE, args=(ticker_id,))
    _drain()

    assert _ticker(issue) == before
    assert _runs(issue) == 0


def test_fire_tick_claim_rolls_back_when_dispatch_fails(api_client):
    """Claim advances used/next_run_at, dispatch returns None (no pod), and
    the rollback restores both — while last_tick_at stays as the attempt
    mark (deliberate, not rolled back)."""
    w, _ = api_client
    issue = _claimable(w, "rollback")
    prev_next = _ticker(issue)["next_run_at"]
    broker.purge()

    broker.publish_task(FIRE, args=(str(_ticker(issue)["id"]),))
    db.wait_for_condition(lambda: _ticker(issue)["last_tick_at"] is not None, what="fire_tick executed")
    _drain()

    tick = _ticker(issue)
    assert tick["last_tick_at"] is not None
    assert tick["used"] == 0
    assert tick["next_run_at"] == prev_next
    assert tick["enabled"] is True
    assert _runs(issue) == 1


def test_fire_tick_unknown_ticker_is_noop(api_client):
    """A ticker id that does not exist: consumed, False, nothing changes."""
    w, _ = api_client
    tickers_before = db.fetchone(get_database_url(), "SELECT count(*) AS n FROM issue_agent_ticker")["n"]
    runs_before = db.fetchone(get_database_url(), "SELECT count(*) AS n FROM agent_run")["n"]
    broker.purge()

    broker.publish_task(FIRE, args=(str(uuid.uuid4()),))
    _drain()

    assert db.fetchone(get_database_url(), "SELECT count(*) AS n FROM issue_agent_ticker")["n"] == tickers_before
    assert db.fetchone(get_database_url(), "SELECT count(*) AS n FROM agent_run")["n"] == runs_before


def test_redelivered_fire_tick_executes_once(api_client):
    """Same task id delivered twice (broker redelivery): the converged ticker
    state is identical to a single delivery — used untouched, one attempt
    mark, no run."""
    w, _ = api_client
    issue = _claimable(w, "redeliver")
    ticker_id = str(_ticker(issue)["id"])
    broker.purge()

    tid = broker.publish_task(FIRE, args=(ticker_id,))
    broker.publish_task(FIRE, args=(ticker_id,), task_id=tid)
    db.wait_for_condition(lambda: _ticker(issue)["last_tick_at"] is not None, what="first delivery executed")
    _drain()

    tick = _ticker(issue)
    assert tick["used"] == 0
    assert tick["enabled"] is True
    assert _runs(issue) == 1


def test_fire_tick_eta_gates_execution(api_client):
    """countdown=ETA: no effect before the ETA, execution after it."""
    w, _ = api_client
    issue = _claimable(w, "eta")
    ticker_id = str(_ticker(issue)["id"])
    broker.purge()

    broker.publish_task(FIRE, args=(ticker_id,), countdown=4)
    import time
    time.sleep(1.5)
    assert _ticker(issue)["last_tick_at"] is None, "executed before its ETA"

    db.wait_for_condition(lambda: _ticker(issue)["last_tick_at"] is not None,
                          timeout=30, what="fire_tick executed after ETA")
    _drain()
    assert _ticker(issue)["used"] == 0
    assert _runs(issue) == 1
