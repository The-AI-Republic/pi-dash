"""Oracle: ``managed_runner.expire_waiting_runs`` (PIDASHCONV-22, D-11).

Source: ``apps/api/pi_dash/managed_runner/tasks.py``.

KNOWN BUG (ported as-is; see the PR): the task is registered in the beat
schedule (``celery.py`` ``managed-runner-expire-waiting-runs``) and covered
by unit tests, but NO installed Django app imports
``pi_dash.managed_runner.tasks`` — unlike ``cloud_agent.tasks``, which
``runner/tasks.py`` imports for autodiscovery. A normal worker therefore
receives the beat publication as an *unregistered* task, discards it, and
the sweep never executes: stale managed waits never expire.

This oracle pins Django-today behavior: publishing the job drains the queue
(the message is consumed, not stuck) but moves no row. The D-11 Rust port
must decide explicitly — reproduce the dead letter or fix the registration —
and this test goes red the moment the behavior changes either way, which is
exactly what the domain gate needs to see.

The sweep *function* itself (QUEUED + older than
``MANAGED_RUNNER_QUEUED_MAX_AGE_SECS`` → ``FAILED``/``desktop_not_connected``,
only QUEUED rows eligible) is pinned by unit tests
(``tests/unit/managed_runner/test_scheduling_and_lifecycle.py``:
``test_sweep_fails_a_wait_that_outlived_the_bound``,
``test_sweep_leaves_a_recent_wait_alone``,
``test_sweep_ignores_other_executors_and_non_queued_rows``).
"""

import pytest

from .conftest import (
    MANAGED_RUNNER,
    QUEUED,
    create_agent_run,
    get_run,
    run_events,
    wait_for_queue_drain,
)

pytestmark = pytest.mark.contract

EXPIRE_TASK = "managed_runner.expire_waiting_runs"
THIRTY_DAYS = 30 * 24 * 3600


def test_unregistered_sweep_has_no_effect(db, broker, seeder, world):
    """Even a month-stale QUEUED managed wait survives publication: the
    worker has no such task registered, so the run row and its events are
    byte-identical before and after."""
    run = create_agent_run(
        seeder, world["workspace"]["id"], world["pod"]["id"], world["owner"]["id"],
        executor_kind=MANAGED_RUNNER, created_ago_seconds=THIRTY_DAYS,
    )
    before = get_run(db, run["id"])
    assert before["status"] == QUEUED
    baseline = broker.queue_length()
    broker.publish(EXPIRE_TASK, [])
    # The message must not pile up: the worker consumes (and discards) it.
    wait_for_queue_drain(broker, baseline=baseline)
    after = get_run(db, run["id"])
    assert after == before
    assert run_events(db, run["id"]) == []
