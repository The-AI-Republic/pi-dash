# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for ``IssueAgentTicker`` — one clock per issue, one budget pool.

The interval is stage-aware (a test cycle ticks slower than a review pass);
the budget is not: ``used`` counts machine-started runs in any stage for the
life of the issue, and the cap is the project pool plus whatever Re-tick
``granted``. See ``.ai_design/ticking_relevance/design.md`` §5 / §9.
"""

from __future__ import annotations

import pytest
from crum import impersonate

from pi_dash.db.models import Issue, Project, State
from pi_dash.db.models.issue_agent_ticker import (
    INFINITE_MAX_TICKS,
    IssueAgentTicker,
)


@pytest.fixture
def project_with_policy(db, workspace, create_user):
    with impersonate(create_user):
        return Project.objects.create(
            name="Web",
            identifier="WEB",
            workspace=workspace,
            created_by=create_user,
            agent_default_interval_seconds=10800,
            agent_default_max_ticks=10,
            agent_review_default_interval_seconds=5400,
            agent_test_default_interval_seconds=7200,
        )


@pytest.fixture
def states(project_with_policy, create_user):
    with impersonate(create_user):
        return {
            "todo": State.objects.create(name="Todo", project=project_with_policy, group="unstarted"),
            "in_progress": State.objects.create(
                name="In Progress", project=project_with_policy, group="started"
            ),
            "in_review": State.objects.create(name="In Review", project=project_with_policy, group="review"),
            "in_test": State.objects.create(name="In Test", project=project_with_policy, group="test"),
            "done": State.objects.create(name="Done", project=project_with_policy, group="completed"),
        }


def _issue_in(workspace, project, states, create_user, key):
    with impersonate(create_user):
        i = Issue.objects.create(
            name="Task",
            workspace=workspace,
            project=project,
            state=states["todo"],
            created_by=create_user,
        )
    Issue.all_objects.filter(pk=i.pk).update(state=states[key])
    i.refresh_from_db()
    return i


@pytest.fixture
def in_progress_issue(workspace, project_with_policy, states, create_user):
    return _issue_in(workspace, project_with_policy, states, create_user, "in_progress")


@pytest.fixture
def in_review_issue(workspace, project_with_policy, states, create_user):
    return _issue_in(workspace, project_with_policy, states, create_user, "in_review")


@pytest.fixture
def in_test_issue(workspace, project_with_policy, states, create_user):
    return _issue_in(workspace, project_with_policy, states, create_user, "in_test")


# ---------------------------------------------------------------------------
# Interval — per stage
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_interval_follows_the_stage(in_progress_issue, in_review_issue, in_test_issue):
    assert IssueAgentTicker.objects.create(issue=in_progress_issue).effective_interval_seconds() == 10800
    assert IssueAgentTicker.objects.create(issue=in_review_issue).effective_interval_seconds() == 5400
    assert IssueAgentTicker.objects.create(issue=in_test_issue).effective_interval_seconds() == 7200


@pytest.mark.unit
def test_interval_re_reads_the_stage_after_a_move(in_progress_issue, states):
    """One row, never rebuilt: moving the issue changes which interval the
    same ticker reads."""
    sched = IssueAgentTicker.objects.create(issue=in_progress_issue)
    assert sched.effective_interval_seconds() == 10800
    Issue.all_objects.filter(pk=in_progress_issue.pk).update(state=states["in_review"])
    in_progress_issue.refresh_from_db()
    sched.issue = in_progress_issue
    assert sched.effective_interval_seconds() == 5400


# ---------------------------------------------------------------------------
# Budget — one pool, any stage
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_cap_is_the_project_pool_regardless_of_stage(in_progress_issue, in_review_issue, in_test_issue):
    for issue in (in_progress_issue, in_review_issue, in_test_issue):
        assert IssueAgentTicker.objects.create(issue=issue).effective_max_ticks() == 10


@pytest.mark.unit
def test_granted_extends_the_pool(in_progress_issue):
    sched = IssueAgentTicker.objects.create(issue=in_progress_issue, used=10, granted=0)
    assert sched.cap_reached() is True
    assert sched.remaining() == 0
    sched.granted = 3
    assert sched.effective_max_ticks() == 13
    assert sched.cap_reached() is False
    assert sched.remaining() == 3


@pytest.mark.unit
def test_used_survives_a_stage_move(in_progress_issue, states):
    """The counter is for the life of the issue: a move to another room of
    the bucket neither resets it nor changes the cap it is measured
    against."""
    sched = IssueAgentTicker.objects.create(issue=in_progress_issue, used=7)
    Issue.all_objects.filter(pk=in_progress_issue.pk).update(state=states["in_review"])
    in_progress_issue.refresh_from_db()
    sched.issue = in_progress_issue
    assert sched.used == 7
    assert sched.effective_max_ticks() == 10
    assert sched.remaining() == 3


@pytest.mark.unit
def test_infinite_pool(in_progress_issue, project_with_policy):
    project_with_policy.agent_default_max_ticks = INFINITE_MAX_TICKS
    project_with_policy.save(update_fields=["agent_default_max_ticks"])
    sched = IssueAgentTicker.objects.create(issue=in_progress_issue, used=999, granted=3)
    assert sched.effective_max_ticks() == INFINITE_MAX_TICKS
    assert sched.cap_reached() is False
    assert sched.remaining() is None


@pytest.mark.unit
def test_tick_count_is_a_read_alias_for_used(in_progress_issue):
    sched = IssueAgentTicker.objects.create(issue=in_progress_issue, used=4)
    assert sched.tick_count == 4


# ---------------------------------------------------------------------------
# Schema defaults
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_project_policy_schema_defaults(db, workspace, create_user):
    with impersonate(create_user):
        project = Project.objects.create(
            name="Policy defaults",
            identifier="POL",
            workspace=workspace,
            created_by=create_user,
        )
    assert project.agent_default_max_ticks == 10
    assert project.agent_default_interval_seconds == 43200
    assert project.agent_review_default_interval_seconds == 28800
    assert project.agent_test_default_interval_seconds == 43200
    # The Re-tick grant is the pool now — the separate knob is gone.
    assert not hasattr(project, "agent_retick_grant")
    assert not hasattr(project, "agent_review_default_max_ticks")
    assert not hasattr(project, "agent_test_default_max_ticks")


@pytest.mark.unit
def test_ticker_row_defaults(in_progress_issue):
    sched = IssueAgentTicker.objects.create(issue=in_progress_issue)
    assert sched.used == 0
    assert sched.granted == 0
    assert sched.pending_entry is False
    assert sched.pending_entry_free is False
    for gone in ("max_ticks", "interval_seconds", "review_max_ticks", "test_max_ticks"):
        assert not hasattr(sched, gone)
