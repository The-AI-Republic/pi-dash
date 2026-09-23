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

# The events are emitted on the name ``settings.LOGGING`` actually
# configures, not on ``getLogger(__name__)`` — see the module comment in
# ``managed_runner/signals.py`` and the delivery test at the bottom of this
# file for why that distinction is the whole point.
EVENT_LOGGER = "pi_dash.managed_runner"


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


def test_run_pinned_logged_when_managed_run_pins_to_a_runner(project, create_user, bundled_runner, caplog):
    with caplog.at_level(logging.INFO, logger=EVENT_LOGGER):
        run = _create_run(
            project,
            create_user,
            executor_kind=AgentExecutorKind.MANAGED_RUNNER,
            pinned_runner=bundled_runner,
        )
    assert f"managed_runner.run_pinned run={run.id} runner={bundled_runner.id}" in caplog.text
    # A pinned live run is not also a waiting one.
    assert "queued_waiting" not in caplog.text


def test_queued_waiting_logged_for_a_parked_automatic_run(project, create_user, bundled_runner, caplog):
    with caplog.at_level(logging.INFO, logger=EVENT_LOGGER):
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
    with caplog.at_level(logging.INFO, logger=EVENT_LOGGER):
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
    with caplog.at_level(logging.INFO, logger=EVENT_LOGGER):
        run.status = AgentRunStatus.RUNNING
        run.save(update_fields=["status"])
    # The events describe creation, not every subsequent save.
    assert caplog.text == ""


@pytest.mark.parametrize("settings_module", ["pi_dash.settings.local", "pi_dash.settings.production"])
def test_event_logger_is_declared_in_settings(settings_module):
    """Regression guard for the defect the first test pass found.

    ``LOGGING`` sets ``disable_existing_loggers: True`` and declares no
    ``root`` logger, so any logger name it does not list resolves to level
    WARNING with zero handlers: the event is computed and then thrown away.
    A ``caplog`` assertion is structurally blind to that, because
    ``caplog.at_level`` attaches its own handler and forces the level. This
    test reads the real settings instead.
    """
    import importlib

    loggers = importlib.import_module(settings_module).LOGGING["loggers"]
    assert EVENT_LOGGER in loggers, (
        f"{settings_module} does not declare {EVENT_LOGGER!r}; managed_runner.* events would be silently dropped"
    )
    config = loggers[EVENT_LOGGER]
    assert config["handlers"], "declared with no handlers — events still go nowhere"
    assert config["level"] in ("INFO", "DEBUG")


def test_run_pinned_reaches_a_handler_under_the_real_logging_config(project, create_user, bundled_runner):
    """End-to-end proof that the event is observable: apply the project's own
    ``LOGGING`` (handlers swapped for an in-memory one) and confirm the record
    the ``post_save`` handler emits actually arrives."""
    import copy
    import importlib
    import logging.config

    records: list[logging.LogRecord] = []

    class _Capture(logging.Handler):
        def emit(self, record):
            records.append(record)

    config = copy.deepcopy(importlib.import_module("pi_dash.settings.local").LOGGING)
    # Leave every other logger in the process alone; this test is only about
    # whether our event name resolves to a handler.
    config["disable_existing_loggers"] = False
    config["handlers"] = {"capture": {"()": _Capture, "level": "DEBUG"}}
    for logger_config in config["loggers"].values():
        logger_config["handlers"] = ["capture"]

    event_logger = logging.getLogger(EVENT_LOGGER)
    try:
        logging.config.dictConfig(config)
        run = _create_run(
            project,
            create_user,
            executor_kind=AgentExecutorKind.MANAGED_RUNNER,
            pinned_runner=bundled_runner,
        )
    finally:
        event_logger.handlers = []
        event_logger.setLevel(logging.NOTSET)
        event_logger.propagate = True

    messages = [r.getMessage() for r in records if "managed_runner.run_pinned" in r.getMessage()]
    assert len(messages) == 1, "the run_pinned event never reached a handler"
    assert f"run={run.id} runner={bundled_runner.id}" in messages[0]


def test_events_never_carry_a_token_or_a_home_path(project, create_user, bundled_runner, caplog):
    """Design §15.1: the events are safe to ship to an aggregator."""
    with caplog.at_level(logging.INFO, logger=EVENT_LOGGER):
        _create_run(
            project,
            create_user,
            executor_kind=AgentExecutorKind.MANAGED_RUNNER,
            pinned_runner=bundled_runner,
        )
        _create_run(
            project,
            create_user,
            executor_kind=AgentExecutorKind.MANAGED_RUNNER,
            pinned_runner=bundled_runner,
            error_code=ManagedRunnerReason.NOT_CONNECTED,
        )
    text = caplog.text
    assert text.strip(), "sanity: the events did fire"
    for forbidden in ("token", "/Users/", "/home/", "CODEX_HOME", "secret"):
        assert forbidden not in text, f"{forbidden!r} leaked into a managed_runner event"
