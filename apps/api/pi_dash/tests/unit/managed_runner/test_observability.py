# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The ``run_pinned`` / ``queued_waiting`` observability events (design §15.1).

Both fire from a single ``post_save(AgentRun)`` handler, so the assertions
create the same rows the real creation path writes and read the log the way an
operator's aggregation would.
"""

from __future__ import annotations

import logging

import pytest

from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.managed_runner.errors import ManagedRunnerReason
from pi_dash.runner.models import AgentRun, AgentRunStatus

pytestmark = pytest.mark.unit

SIGNAL_LOGGER = "pi_dash.managed_runner.signals"


def _create_run(project, owner, *, executor_kind, pinned_runner=None, error_code=""):
    return AgentRun.objects.create(
        workspace=project.workspace,
        created_by=owner,
        pod=project.pods.get(is_default=True),
        executor_kind=executor_kind,
        pinned_runner=pinned_runner,
        status=AgentRunStatus.QUEUED,
        error_code=error_code,
        prompt="",
    )


def test_run_pinned_logged_when_managed_run_pins_to_a_runner(
    project, create_user, bundled_runner, caplog
):
    with caplog.at_level(logging.INFO, logger=SIGNAL_LOGGER):
        run = _create_run(
            project,
            create_user,
            executor_kind=AgentExecutorKind.MANAGED_RUNNER,
            pinned_runner=bundled_runner,
        )
    assert f"managed_runner.run_pinned run={run.id} runner={bundled_runner.id}" in caplog.text
    # A pinned live run is not also a waiting one.
    assert "queued_waiting" not in caplog.text


def test_queued_waiting_logged_for_a_parked_automatic_run(
    project, create_user, bundled_runner, caplog
):
    with caplog.at_level(logging.INFO, logger=SIGNAL_LOGGER):
        run = _create_run(
            project,
            create_user,
            executor_kind=AgentExecutorKind.MANAGED_RUNNER,
            pinned_runner=bundled_runner,
            error_code=ManagedRunnerReason.NOT_CONNECTED,
        )
    assert f"managed_runner.queued_waiting run={run.id} runner={bundled_runner.id}" in caplog.text
    # Waiting is not pinning: the two events are mutually exclusive.
    assert "run_pinned" not in caplog.text


def test_no_event_for_non_managed_runs(project, create_user, caplog):
    with caplog.at_level(logging.INFO, logger=SIGNAL_LOGGER):
        _create_run(
            project,
            create_user,
            executor_kind=AgentExecutorKind.LOCAL_RUNNER,
        )
    assert caplog.text == ""


def test_no_event_on_update_of_a_managed_run(project, create_user, bundled_runner, caplog):
    run = _create_run(
        project,
        create_user,
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
        pinned_runner=bundled_runner,
    )
    with caplog.at_level(logging.INFO, logger=SIGNAL_LOGGER):
        run.status = AgentRunStatus.RUNNING
        run.save(update_fields=["status"])
    # The events describe creation, not every subsequent save.
    assert caplog.text == ""
