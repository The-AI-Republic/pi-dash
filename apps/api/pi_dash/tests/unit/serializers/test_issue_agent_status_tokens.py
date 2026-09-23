# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""``agent_status.total_tokens`` — the issue's cumulative token total
(PDASHOSS01-189).

The per-run numbers were already on the payload; what nothing exposed was the
issue-level rollup the agent-status tile needs. ``AgentRun.total_tokens`` is a
generated column over ``AgentRun.usage`` (PDASHOSS01-188), so this is a plain
``Sum`` over a real column.
"""

import pytest

from pi_dash.app.serializers.issue import IssueDetailSerializer
from pi_dash.db.models import Issue, State
from pi_dash.runner.models import AgentRun, AgentRunStatus, Pod


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def issue(workspace, project, create_user):
    # Deliberately an unstarted state: moving an issue into a ticking state
    # asks orchestration to create a run, which would make ``run_count``
    # depend on something this test isn't about.
    state = State.objects.create(name="Todo", project=project, group="unstarted")
    return Issue.objects.create(
        name="Show token consumption", workspace=workspace, project=project, state=state, created_by=create_user
    )


def _run(issue, workspace, pod, create_user, usage, status=AgentRunStatus.COMPLETED):
    # ``agent_run_one_active_per_work_item`` allows only one non-terminal run
    # per issue, so a history of runs is a history of terminal ones.
    return AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        work_item=issue,
        usage=usage,
        status=status,
    )


def _agent_status(issue):
    return IssueDetailSerializer(issue).data["agent_status"]


@pytest.mark.unit
class TestIssueAgentStatusTotalTokens:
    def test_sums_every_run_on_the_issue(self, issue, workspace, pod, create_user):
        _run(issue, workspace, pod, create_user, {"input": 100, "output": 40, "total": 140})
        _run(issue, workspace, pod, create_user, {"input": 8_000, "output": 660, "total": 8_660})

        status = _agent_status(issue)
        assert status["run_count"] == 2
        assert status["total_tokens"] == 8_800

    def test_a_run_that_never_reported_usage_counts_as_zero_not_null(self, issue, workspace, pod, create_user):
        # A run that died before reporting stores NULL. The sum has to skip it
        # rather than come back None, or the tile renders blank.
        _run(issue, workspace, pod, create_user, {"input": 100, "output": 40, "total": 140})
        _run(issue, workspace, pod, create_user, {})

        status = _agent_status(issue)
        assert status["run_count"] == 2
        assert status["total_tokens"] == 140

    def test_reports_zero_when_no_run_ever_reported_usage(self, issue, workspace, pod, create_user):
        _run(issue, workspace, pod, create_user, {})

        status = _agent_status(issue)
        assert status["run_count"] == 1
        assert status["total_tokens"] == 0

    def test_includes_a_run_still_in_flight_once_its_row_carries_usage(self, issue, workspace, pod, create_user):
        # A running row has no usage until it pauses or ends, so it adds
        # nothing yet — the tile leans on the live state for that. Once the
        # row is written the sum picks it up with no further work.
        _run(issue, workspace, pod, create_user, {"input": 100, "output": 40, "total": 140})
        running = _run(issue, workspace, pod, create_user, {}, status=AgentRunStatus.RUNNING)

        assert _agent_status(issue)["total_tokens"] == 140

        AgentRun.objects.filter(pk=running.pk).update(usage={"input": 500, "output": 60, "total": 560})
        status = _agent_status(issue)
        assert status["run_count"] == 2
        assert status["total_tokens"] == 700

    def test_counts_only_this_issues_runs(self, issue, workspace, project, pod, create_user):
        other = Issue.objects.create(
            name="Someone else's issue",
            workspace=workspace,
            project=project,
            state=issue.state,
            created_by=create_user,
        )
        _run(issue, workspace, pod, create_user, {"input": 100, "output": 40, "total": 140})
        _run(other, workspace, pod, create_user, {"input": 9_000, "output": 900, "total": 9_900})

        assert _agent_status(issue)["total_tokens"] == 140
        assert _agent_status(other)["total_tokens"] == 9_900
