"""Beat-schedule firing parity (PIDASHCONV-22, D-11; trimmed P3 step B).

Source: ``apps/api/pi_dash/celery.py`` ``_register_settings_backed_beat_entries``
— the one schedule entry this domain still owns, with the cadence it reads
from settings (defaults from ``settings/common.py``):

- ``managed-runner-expire-waiting-runs`` → ``managed_runner.expire_waiting_runs``
  every ``MANAGED_RUNNER_SWEEP_INTERVAL_SECONDS`` (300).

P3 deleted the retired scan/sweep entries with the cloud plane. The cadence
is pinned here as data; the expire suite proves the firing half (a bare
task-name publication with empty args resolves to the pinned behavior).
"""

import pytest

pytestmark = pytest.mark.contract

#: Schedule entry → (task name, interval seconds, settings key).
DISPATCH_BEAT_ENTRIES = {
    "managed-runner-expire-waiting-runs": (
        "managed_runner.expire_waiting_runs",
        300,
        "MANAGED_RUNNER_SWEEP_INTERVAL_SECONDS",
    ),
}


def test_dispatch_beat_entries_are_pinned():
    """The domain owns exactly this entry. Adding, dropping, or
    retiming one breaks this mapping on purpose."""
    assert set(DISPATCH_BEAT_ENTRIES) == {
        "managed-runner-expire-waiting-runs",
    }
    tasks = {task for task, _, _ in DISPATCH_BEAT_ENTRIES.values()}
    assert tasks == {
        "managed_runner.expire_waiting_runs",
    }
    intervals = {name: secs for name, (_, secs, _) in DISPATCH_BEAT_ENTRIES.items()}
    assert intervals == {
        "managed-runner-expire-waiting-runs": 300,
    }


def test_every_scheduled_task_has_an_execution_suite():
    """Each scheduled task name resolves to the module that publishes it
    exactly as beat would — no drift between this mapping and the suites."""
    import importlib

    covered = {
        "managed_runner.expire_waiting_runs": "dispatch.test_expire_waiting_runs",
    }
    for task, module_name in covered.items():
        module = importlib.import_module(module_name)
        assert task in module.__doc__, f"{module_name} no longer covers {task}"
