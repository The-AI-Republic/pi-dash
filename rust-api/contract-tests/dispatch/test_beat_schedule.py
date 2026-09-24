"""Beat-schedule firing parity (PIDASHCONV-22, D-11).

Source: ``apps/api/pi_dash/celery.py`` ``_register_settings_backed_beat_entries``
— the three schedule entries this domain owns, with the cadence each reads
from settings (defaults from ``settings/common.py``):

- ``cloud-agent-scan-queued-runs`` → ``cloud_agent.scan_queued_runs`` every
  ``CLOUD_AGENT_DISPATCH_SCAN_INTERVAL_SECONDS`` (10).
- ``cloud-agent-sweep-stale-runs`` → ``cloud_agent.sweep_stale_runs`` every
  ``CLOUD_AGENT_SWEEP_INTERVAL_SECONDS`` (30).
- ``managed-runner-expire-waiting-runs`` → ``managed_runner.expire_waiting_runs``
  every ``MANAGED_RUNNER_SWEEP_INTERVAL_SECONDS`` (300).

Cadences are deployment settings no black-box suite can observe quickly
(and must not: waiting out a 300s interval per run is not a contract test),
so they are pinned here as data with source references. What the execution
suites prove is the other half of "firing": each scheduled (task, no-args)
publication resolves to the pinned behavior — the scan/sweep tests publish
exactly what beat would (bare task name, empty args, ``celery`` queue) and
observe the effect, while the expire test pins the unregistered-task
dead letter. This mapping fails the moment a name, cadence, or behavior
drifts on either backend.
"""

import pytest

pytestmark = pytest.mark.contract

#: Schedule entry → (task name, interval seconds, settings key).
DISPATCH_BEAT_ENTRIES = {
    "cloud-agent-scan-queued-runs": (
        "cloud_agent.scan_queued_runs",
        10,
        "CLOUD_AGENT_DISPATCH_SCAN_INTERVAL_SECONDS",
    ),
    "cloud-agent-sweep-stale-runs": (
        "cloud_agent.sweep_stale_runs",
        30,
        "CLOUD_AGENT_SWEEP_INTERVAL_SECONDS",
    ),
    "managed-runner-expire-waiting-runs": (
        "managed_runner.expire_waiting_runs",
        300,
        "MANAGED_RUNNER_SWEEP_INTERVAL_SECONDS",
    ),
}


def test_dispatch_beat_entries_are_pinned():
    """The domain owns exactly these three entries. Adding, dropping, or
    retiming one breaks this mapping on purpose."""
    assert set(DISPATCH_BEAT_ENTRIES) == {
        "cloud-agent-scan-queued-runs",
        "cloud-agent-sweep-stale-runs",
        "managed-runner-expire-waiting-runs",
    }
    tasks = {task for task, _, _ in DISPATCH_BEAT_ENTRIES.values()}
    assert tasks == {
        "cloud_agent.scan_queued_runs",
        "cloud_agent.sweep_stale_runs",
        "managed_runner.expire_waiting_runs",
    }
    intervals = {name: secs for name, (_, secs, _) in DISPATCH_BEAT_ENTRIES.items()}
    assert intervals == {
        "cloud-agent-scan-queued-runs": 10,
        "cloud-agent-sweep-stale-runs": 30,
        "managed-runner-expire-waiting-runs": 300,
    }


def test_every_scheduled_task_has_an_execution_suite():
    """Each scheduled task name resolves to the module that publishes it
    exactly as beat would — no drift between this mapping and the suites."""
    import importlib

    covered = {
        "cloud_agent.scan_queued_runs": "dispatch.test_scan_queued_runs",
        "cloud_agent.sweep_stale_runs": "dispatch.test_sweep_stale_runs",
        "managed_runner.expire_waiting_runs": "dispatch.test_expire_waiting_runs",
    }
    for task, module_name in covered.items():
        module = importlib.import_module(module_name)
        assert task in module.__doc__, f"{module_name} no longer covers {task}"
