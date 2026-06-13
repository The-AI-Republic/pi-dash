# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""AgentRun.trigger classification (design §9.1)."""

from __future__ import annotations

import pytest

from pi_dash.runner.models import AgentRun, AgentRunTrigger, run_is_human_triggered


@pytest.mark.unit
def test_default_trigger_is_direct(db, workspace, create_user):
    run = AgentRun.objects.create(workspace=workspace, prompt="", created_by=create_user)
    assert run.trigger == AgentRunTrigger.DIRECT
    assert run_is_human_triggered(run)


@pytest.mark.unit
@pytest.mark.parametrize(
    "trigger,expected",
    [
        (AgentRunTrigger.STATE_TRANSITION, True),
        (AgentRunTrigger.RUN_AI, True),
        (AgentRunTrigger.COMMENT_AND_RUN, True),
        (AgentRunTrigger.DIRECT, True),
        (AgentRunTrigger.TICK, False),
        (AgentRunTrigger.SCHEDULER, False),
    ],
)
def test_human_triggered_classification(db, workspace, create_user, trigger, expected):
    run = AgentRun.objects.create(
        workspace=workspace, prompt="", trigger=trigger, created_by=create_user
    )
    assert run_is_human_triggered(run) is expected


@pytest.mark.unit
def test_automatic_run_resolves_no_user_overrides(db, workspace, create_user):
    """A tick/scheduler run must NOT pick up the creator's personal overrides:
    the composer passes user=None for non-human triggers."""
    from pi_dash.prompting.composer import _user_for_run

    human = AgentRun.objects.create(
        workspace=workspace, prompt="", trigger=AgentRunTrigger.RUN_AI, created_by=create_user
    )
    auto = AgentRun.objects.create(
        workspace=workspace, prompt="", trigger=AgentRunTrigger.TICK, created_by=create_user
    )
    assert _user_for_run(human) == create_user
    assert _user_for_run(auto) is None
