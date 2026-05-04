# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Regression: ``finalize_run_terminal`` posts an IssueComment on FAILED.

Today the issue activity feed is the only UI a user is guaranteed to
see. If a run fails for any reason — agent stalled, codex crashed,
git auth — and the cloud only writes ``AgentRun.error`` to the row,
the user has no in-product signal that anything went wrong: the run
just disappears from "running" without explanation.

These tests pin down two invariants:

1. A FAILED finalize posts exactly one IssueComment with the runner's
   error_detail rendered into it.
2. A successful finalize (COMPLETED) does NOT post a comment — the
   normal completion path is responsible for its own UX.

Comment-posting failures must never block the lifecycle terminal
transition; that's covered by the `_post_failure_comment_swallows_errors`
test.
"""

from __future__ import annotations

from unittest.mock import patch

import pytest
from django.utils import timezone

from pi_dash.db.models.issue import Issue, IssueComment
from pi_dash.db.models.state import State
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services.run_lifecycle import finalize_run_terminal


# ---------------------------------------------------------------------------
# Fixtures (mirrored from test_runner_live_state.py / test_composer.py)
# ---------------------------------------------------------------------------


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def state(project):
    return State.objects.create(
        name="Todo",
        project=project,
        group="unstarted",
    )


@pytest.fixture
def issue(workspace, project, state, create_user):
    return Issue.objects.create(
        name="failure-comment-test",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
        priority="medium",
    )


def _make_runner(user, workspace, pod, name="r1"):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


def _make_run(user, workspace, pod, runner, issue, *, status=AgentRunStatus.RUNNING):
    return AgentRun.objects.create(
        workspace=workspace,
        owner=user,
        created_by=user,
        pod=pod,
        runner=runner,
        work_item=issue,
        status=status,
        prompt="test",
        assigned_at=timezone.now(),
        started_at=timezone.now(),
    )


@pytest.fixture(autouse=True)
def _run_on_commit_immediately():
    """The lifecycle helper schedules drain work via on_commit; tests run
    outside an atomic block so the callbacks would otherwise never fire."""
    with patch(
        "django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()
    ):
        yield


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_failed_finalize_posts_failure_comment(
    db, create_user, workspace, pod, issue
):
    """Asserts the new behaviour: a FAILED finalize creates one comment
    on the issue with the runner's error_detail visible inside it."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)

    detail = (
        "no agent frames for 5 minutes; last command: `git fetch origin` "
        "in `/tmp/x` (started 297s ago)"
    )
    finalize_run_terminal(
        runner, run.id, AgentRunStatus.FAILED, error_detail=detail
    )

    comments = list(IssueComment.objects.filter(issue=issue))
    assert len(comments) == 1, f"expected 1 comment, got {len(comments)}"
    body = comments[0].comment_html
    assert "Run failed" in body
    # The detail string must be visible to the user — that's the whole
    # point of the comment. HTML escaping may transform backticks but the
    # essential context (the cmd + the stall) must remain.
    assert "git fetch origin" in body
    assert "5 minutes" in body


@pytest.mark.unit
def test_failed_finalize_renders_multiline_stderr_tail(
    db, create_user, workspace, pod, issue
):
    """Real failure details are multi-line: classifier + last cmd +
    stderr tail joined with `\\n  `. The whole tail must end up in the
    rendered comment so the user can see what the agent was complaining
    about, not just the headline."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)

    detail = (
        "no agent frames for 5 minutes; "
        "last command: `npm install` (started 297s ago); "
        "stderr tail (3 line(s)):\n"
        "  npm warn deprecated foo@1.0.0\n"
        "  npm err! ENOTFOUND registry.npmjs.org\n"
        "  npm err! exiting with code 1"
    )
    finalize_run_terminal(
        runner, run.id, AgentRunStatus.FAILED, error_detail=detail
    )

    body = IssueComment.objects.get(issue=issue).comment_html
    for line in [
        "npm warn deprecated foo@1.0.0",
        "npm err! ENOTFOUND registry.npmjs.org",
        "npm err! exiting with code 1",
    ]:
        assert line in body, f"stderr line missing from comment: {line!r}"


@pytest.mark.unit
def test_failure_comment_actor_is_agent_system_user(
    db, create_user, workspace, pod, issue
):
    """Pin the actor identity. If a future refactor sets `actor` to the
    run's owner instead, the user would see themselves complaining
    about their own task failing in the activity feed."""
    from pi_dash.orchestration.workpad import get_agent_system_user

    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)
    finalize_run_terminal(
        runner, run.id, AgentRunStatus.FAILED, error_detail="boom"
    )
    comment = IssueComment.objects.get(issue=issue)
    assert comment.actor_id == get_agent_system_user().id


@pytest.mark.unit
def test_daemon_restart_failure_does_not_post_comment(
    db, create_user, workspace, pod, issue
):
    """Infrastructure-flavored failures (the runner went down for
    SIGTERM) shouldn't surface on the issue thread — there's nothing
    the user can act on, and the next continuation will pick up the
    work. The DB row still gets the error stamp."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)

    finalize_run_terminal(
        runner,
        run.id,
        AgentRunStatus.FAILED,
        error_detail="daemon shutdown requested",
    )

    assert IssueComment.objects.filter(issue=issue).count() == 0
    run.refresh_from_db()
    assert run.status == AgentRunStatus.FAILED
    assert run.error == "daemon shutdown requested"


@pytest.mark.unit
def test_cloud_stall_reconciler_failure_does_not_post_comment(
    db, create_user, workspace, pod, issue
):
    """The cloud-side stall watchdog (`reconcile_stalled_runs`) emits
    `agent stalled: no events for >360s` — same suppression class as
    daemon-restart."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)

    finalize_run_terminal(
        runner,
        run.id,
        AgentRunStatus.FAILED,
        error_detail="agent stalled: no events for >360s",
    )

    assert IssueComment.objects.filter(issue=issue).count() == 0


@pytest.mark.unit
def test_completed_finalize_does_not_post_comment(
    db, create_user, workspace, pod, issue
):
    """COMPLETED finalize must not post a failure comment — the success
    path has its own UX and we shouldn't double-comment."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)

    finalize_run_terminal(
        runner,
        run.id,
        AgentRunStatus.COMPLETED,
        done_payload={"conclusion": "success"},
    )

    assert IssueComment.objects.filter(issue=issue).count() == 0


@pytest.mark.unit
def test_post_failure_comment_swallows_errors(
    db, create_user, workspace, pod, issue
):
    """If comment posting raises (DB hiccup, missing system user, etc.),
    `finalize_run_terminal` must still complete the lifecycle update.
    A failure-comment crash cannot leave runs stuck in non-terminal
    states or block the pod's drain re-fire."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)

    with patch(
        "pi_dash.runner.services.run_lifecycle._post_failure_comment",
        side_effect=RuntimeError("simulated outage"),
    ):
        finalize_run_terminal(
            runner, run.id, AgentRunStatus.FAILED, error_detail="boom"
        )

    run.refresh_from_db()
    assert run.status == AgentRunStatus.FAILED
    assert run.error == "boom"
    assert run.ended_at is not None


@pytest.mark.unit
def test_failed_finalize_with_orphan_run_no_workitem_does_not_crash(
    db, create_user, workspace, pod
):
    """Some failed runs (e.g. ad-hoc / synthetic) carry no work_item.
    The comment helper must short-circuit cleanly in that case."""
    runner = _make_runner(create_user, workspace, pod)
    run = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        runner=runner,
        work_item=None,
        status=AgentRunStatus.RUNNING,
        prompt="orphan",
        assigned_at=timezone.now(),
        started_at=timezone.now(),
    )

    finalize_run_terminal(
        runner, run.id, AgentRunStatus.FAILED, error_detail="orphan failure"
    )

    run.refresh_from_db()
    assert run.status == AgentRunStatus.FAILED
    # Total comment count across the whole DB doesn't change for a run
    # with no work_item.
    assert IssueComment.objects.count() == 0
