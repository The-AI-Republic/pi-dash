# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for ``pi_dash.orchestration.agent_phases``.

The registry is the source of truth for which states tick and which
prompt template each ticking phase uses. Coverage is exhaustive over
every ``StateGroup`` value so adding a new group later is forced to
also decide whether it ticks.
"""

from __future__ import annotations

import pytest

from pi_dash.db.models.state import StateGroup
from pi_dash.orchestration import agent_phases
from pi_dash.prompting.models import PromptTemplate


class _StubState:
    def __init__(self, group: str, name: str):
        self.group = group
        self.name = name


@pytest.mark.unit
def test_phases_contains_started_in_progress():
    cfg = agent_phases.PHASES[StateGroup.STARTED.value]
    assert cfg.state_name == "In Progress"
    assert cfg.template_name == PromptTemplate.DEFAULT_NAME
    assert cfg.fresh_session_on_entry is False


@pytest.mark.unit
def test_phases_contains_review_in_review():
    cfg = agent_phases.PHASES[StateGroup.REVIEW.value]
    assert cfg.state_name == "In Review"
    assert cfg.template_name == "review"
    assert cfg.fresh_session_on_entry is True
    assert cfg.disarm_on_completed is True


@pytest.mark.unit
def test_is_ticking_state_true_for_review_in_review():
    state = _StubState(StateGroup.REVIEW.value, "In Review")
    assert agent_phases.is_ticking_state(state) is True


@pytest.mark.unit
def test_template_name_for_review_in_review_is_review():
    name = agent_phases.template_name_for(
        _StubState(StateGroup.REVIEW.value, "In Review")
    )
    assert name == "review"


@pytest.mark.unit
def test_is_ticking_state_true_for_started_in_progress():
    state = _StubState(StateGroup.STARTED.value, "In Progress")
    assert agent_phases.is_ticking_state(state) is True


@pytest.mark.unit
def test_is_ticking_state_false_for_started_with_custom_name():
    # Workspaces with custom state names within a ticking group still
    # do not tick — the registry pins the literal state name per group.
    state = _StubState(StateGroup.STARTED.value, "Doing")
    assert agent_phases.is_ticking_state(state) is False


@pytest.mark.unit
def test_is_ticking_state_false_for_none():
    assert agent_phases.is_ticking_state(None) is False


@pytest.mark.unit
@pytest.mark.parametrize(
    "group",
    [
        g.value
        for g in StateGroup
        if g.value not in agent_phases.PHASES
    ],
)
def test_is_ticking_state_false_for_unregistered_groups(group):
    # All non-registered groups stay non-ticking — backlog, unstarted,
    # completed, cancelled, triage today.
    assert agent_phases.is_ticking_state(_StubState(group, "any")) is False


@pytest.mark.unit
def test_phase_config_for_returns_started_config():
    cfg = agent_phases.phase_config_for(
        _StubState(StateGroup.STARTED.value, "In Progress")
    )
    assert cfg is not None
    assert cfg.state_name == "In Progress"


@pytest.mark.unit
def test_phase_config_for_none_when_state_is_none():
    assert agent_phases.phase_config_for(None) is None


@pytest.mark.unit
def test_phase_config_for_none_when_state_name_doesnt_match():
    cfg = agent_phases.phase_config_for(
        _StubState(StateGroup.STARTED.value, "Doing")
    )
    assert cfg is None


@pytest.mark.unit
def test_template_name_for_started_in_progress_is_coding_task():
    name = agent_phases.template_name_for(
        _StubState(StateGroup.STARTED.value, "In Progress")
    )
    assert name == PromptTemplate.DEFAULT_NAME == "coding-task"


@pytest.mark.unit
def test_template_name_for_unregistered_state_falls_back_to_default():
    name = agent_phases.template_name_for(
        _StubState(StateGroup.COMPLETED.value, "Done")
    )
    assert name == PromptTemplate.DEFAULT_NAME
