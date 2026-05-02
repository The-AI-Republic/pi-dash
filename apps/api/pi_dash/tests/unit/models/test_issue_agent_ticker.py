# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the phase-aware ``effective_interval_seconds`` and
``effective_max_ticks`` on ``IssueAgentTicker``.

The resolver picks the In Review pair when the issue's current state
is the In Review phase and the In Progress pair otherwise. Each pair
falls through: per-issue override → project default → constant.

See ``.ai_design/create_review_state/design.md`` §6.4.
"""

from __future__ import annotations

import pytest
from crum import impersonate

from pi_dash.db.models import Issue, Project, State
from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker


@pytest.fixture
def project_with_overrides(db, workspace, create_user):
    with impersonate(create_user):
        return Project.objects.create(
            name="Web",
            identifier="WEB",
            workspace=workspace,
            created_by=create_user,
            agent_default_interval_seconds=10800,
            agent_default_max_ticks=24,
            agent_review_default_interval_seconds=10800,
            agent_review_default_max_ticks=8,
        )


@pytest.fixture
def states(project_with_overrides, create_user):
    with impersonate(create_user):
        return {
            "todo": State.objects.create(
                name="Todo", project=project_with_overrides, group="unstarted"
            ),
            "in_progress": State.objects.create(
                name="In Progress",
                project=project_with_overrides,
                group="started",
            ),
            "in_review": State.objects.create(
                name="In Review",
                project=project_with_overrides,
                group="review",
            ),
            "done": State.objects.create(
                name="Done",
                project=project_with_overrides,
                group="completed",
            ),
        }


@pytest.fixture
def in_progress_issue(workspace, project_with_overrides, states, create_user):
    with impersonate(create_user):
        i = Issue.objects.create(
            name="Task",
            workspace=workspace,
            project=project_with_overrides,
            state=states["todo"],
            created_by=create_user,
        )
    Issue.all_objects.filter(pk=i.pk).update(state=states["in_progress"])
    i.refresh_from_db()
    return i


@pytest.fixture
def in_review_issue(workspace, project_with_overrides, states, create_user):
    with impersonate(create_user):
        i = Issue.objects.create(
            name="Task",
            workspace=workspace,
            project=project_with_overrides,
            state=states["todo"],
            created_by=create_user,
        )
    Issue.all_objects.filter(pk=i.pk).update(state=states["in_review"])
    i.refresh_from_db()
    return i


@pytest.mark.unit
def test_in_progress_uses_impl_project_defaults(in_progress_issue):
    sched = IssueAgentTicker.objects.create(issue=in_progress_issue)
    assert sched.effective_interval_seconds() == 10800
    assert sched.effective_max_ticks() == 24


@pytest.mark.unit
def test_in_review_uses_review_project_defaults(in_review_issue):
    sched = IssueAgentTicker.objects.create(issue=in_review_issue)
    assert sched.effective_interval_seconds() == 10800
    # Review-phase cap defaults to 8 (24h window) — distinct from impl's 24.
    assert sched.effective_max_ticks() == 8


@pytest.mark.unit
def test_in_progress_override_wins_over_project_default(in_progress_issue):
    sched = IssueAgentTicker.objects.create(
        issue=in_progress_issue,
        interval_seconds=900,
        max_ticks=100,
    )
    assert sched.effective_interval_seconds() == 900
    assert sched.effective_max_ticks() == 100


@pytest.mark.unit
def test_in_review_override_wins_over_project_default(in_review_issue):
    sched = IssueAgentTicker.objects.create(
        issue=in_review_issue,
        review_interval_seconds=600,
        review_max_ticks=4,
    )
    assert sched.effective_interval_seconds() == 600
    assert sched.effective_max_ticks() == 4


@pytest.mark.unit
def test_review_phase_ignores_impl_override(in_review_issue):
    """Per-issue impl overrides do NOT apply when the ticker is
    currently In Review — and vice versa. The two pairs are
    independent."""
    sched = IssueAgentTicker.objects.create(
        issue=in_review_issue,
        interval_seconds=900,  # impl override — must NOT apply
        max_ticks=100,         # impl override — must NOT apply
    )
    assert sched.effective_interval_seconds() == 10800
    assert sched.effective_max_ticks() == 8


@pytest.mark.unit
def test_in_progress_phase_ignores_review_override(in_progress_issue):
    sched = IssueAgentTicker.objects.create(
        issue=in_progress_issue,
        review_interval_seconds=600,  # review override — must NOT apply
        review_max_ticks=4,           # review override — must NOT apply
    )
    assert sched.effective_interval_seconds() == 10800
    assert sched.effective_max_ticks() == 24


@pytest.mark.unit
def test_zero_or_negative_interval_falls_through(in_progress_issue):
    """The override-validity check is ``> 0`` so a misconfigured 0
    falls back to the project default."""
    sched = IssueAgentTicker.objects.create(
        issue=in_progress_issue,
        interval_seconds=0,
    )
    assert sched.effective_interval_seconds() == 10800


@pytest.mark.unit
def test_explicit_zero_max_ticks_is_preserved(in_progress_issue):
    """``max_ticks`` is preserved at 0 (intentional "no ticks") because
    the override-validity check uses ``is not None`` rather than
    ``> 0``. Compare with ``effective_interval_seconds`` where 0 falls
    through — the asymmetry is intentional: an explicit 0 cap is a
    valid kill switch, but a 0 interval would fire continuously and is
    treated as a misconfiguration.
    """
    sched = IssueAgentTicker.objects.create(
        issue=in_progress_issue,
        max_ticks=0,
    )
    assert sched.effective_max_ticks() == 0


@pytest.mark.unit
def test_explicit_zero_review_max_ticks_is_preserved(in_review_issue):
    """Same kill-switch semantics on the review-phase override pair."""
    sched = IssueAgentTicker.objects.create(
        issue=in_review_issue,
        review_max_ticks=0,
    )
    assert sched.effective_max_ticks() == 0
