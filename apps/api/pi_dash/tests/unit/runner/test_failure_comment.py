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
    RunnerLiveState,
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
def test_agent_auth_failure_persists_actionable_cloud_error(
    db, create_user, workspace, pod, issue
):
    runner = _make_runner(create_user, workspace, pod, name="workx_claude01")
    runner.capabilities = ["agent:claude_code"]
    runner.save(update_fields=["capabilities"])
    run = _make_run(create_user, workspace, pod, runner, issue)
    raw = "Failed to authenticate. API Error: 401 Invalid authentication credentials"

    finalize_run_terminal(
        runner,
        run.id,
        AgentRunStatus.FAILED,
        error_detail=raw,
    )

    run.refresh_from_db()
    assert run.error.startswith("401 authentication_failed\n")
    assert (
        'AI agent: Claude Code auth appears expired or invalid. Go to the dev machine for runner "workx_claude01" '
        "and re-authenticate Claude Code"
    ) in run.error
    assert f"Raw agent error:\n{raw}" in run.error


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


@pytest.mark.unit
def test_finalize_persists_direct_usage_metadata(
    db, create_user, workspace, pod, issue
):
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)

    finalize_run_terminal(
        runner,
        run.id,
        AgentRunStatus.COMPLETED,
        done_payload={"conclusion": "success"},
        tokens={"input": 1000, "output": 250, "total": 1250},
        model="gpt-5.1-codex",
    )

    run.refresh_from_db()
    assert run.status == AgentRunStatus.COMPLETED
    assert run.llm_model == "gpt-5.1-codex"
    assert run.input_tokens == 1000
    assert run.output_tokens == 250
    assert run.total_tokens == 1250


@pytest.mark.unit
def test_finalize_ignores_out_of_range_usage_metadata(
    db, create_user, workspace, pod, issue
):
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)

    finalize_run_terminal(
        runner,
        run.id,
        AgentRunStatus.COMPLETED,
        done_payload={"conclusion": "success"},
        tokens={"input": 2**63, "output": 250, "total": 2**63},
    )

    run.refresh_from_db()
    assert run.status == AgentRunStatus.COMPLETED
    assert run.input_tokens is None
    assert run.output_tokens == 250
    assert run.total_tokens is None


@pytest.mark.unit
def test_finalize_persists_done_payload_usage_metadata(
    db, create_user, workspace, pod, issue
):
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)

    finalize_run_terminal(
        runner,
        run.id,
        AgentRunStatus.COMPLETED,
        done_payload={
            "conclusion": "success",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 30,
                "total_tokens": 130,
            },
        },
    )

    run.refresh_from_db()
    assert run.status == AgentRunStatus.COMPLETED
    assert run.input_tokens == 100
    assert run.output_tokens == 30
    assert run.total_tokens == 130


@pytest.mark.unit
def test_finalize_falls_back_to_matching_live_state_usage(
    db, create_user, workspace, pod, issue
):
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)
    RunnerLiveState.objects.create(
        runner=runner,
        observed_run_id=run.id,
        usage={"input": 10, "output": 20, "total": 30},
        llm_model="claude-sonnet-4-6",
    )

    finalize_run_terminal(
        runner,
        run.id,
        AgentRunStatus.CANCELLED,
    )

    run.refresh_from_db()
    assert run.status == AgentRunStatus.CANCELLED
    assert run.llm_model == "claude-sonnet-4-6"
    assert run.input_tokens == 10
    assert run.output_tokens == 20
    assert run.total_tokens == 30


# ---------------------------------------------------------------------------
# Canonical failure_reason at write time (PDASHOSS01-183)
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_failed_finalize_stores_canonical_failure_reason(
    db, create_user, workspace, pod, issue
):
    """The runner's coarse reason + the error text classify into the
    stored taxonomy at write time."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)

    finalize_run_terminal(
        runner,
        run.id,
        AgentRunStatus.FAILED,
        error_detail="401 authentication_failed: invalid authentication credentials",
        runner_failure_reason="agent_crash",
    )

    run.refresh_from_db()
    assert run.failure_reason == "agent_error.provider_auth_or_access"


@pytest.mark.unit
def test_failed_finalize_maps_platform_runner_reason_directly(
    db, create_user, workspace, pod, issue
):
    """Old runners only send today's FailureReason values — they map
    platform-side without consulting the text rules."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)

    finalize_run_terminal(
        runner,
        run.id,
        AgentRunStatus.FAILED,
        error_detail="could not push: authentication failed for origin",
        runner_failure_reason="git_auth",
    )

    run.refresh_from_db()
    assert run.failure_reason == "git_auth"


@pytest.mark.unit
def test_failed_finalize_without_runner_reason_still_classifies(
    db, create_user, workspace, pod, issue
):
    """The finalize safety net keeps the invariant for writers that have
    no structured reason at all (legacy runners, cloud reapers)."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)

    finalize_run_terminal(
        runner,
        run.id,
        AgentRunStatus.FAILED,
        error_detail="agent stalled: no events for >360s",
    )

    run.refresh_from_db()
    assert run.failure_reason == "timeout"


@pytest.mark.unit
def test_completed_finalize_leaves_failure_reason_blank(
    db, create_user, workspace, pod, issue
):
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)
    finalize_run_terminal(runner, run.id, AgentRunStatus.COMPLETED, done_payload={"status": "done"})
    run.refresh_from_db()
    assert run.failure_reason == ""


@pytest.mark.unit
def test_needs_human_failure_comment_names_the_fix_and_the_stopped_clock(
    db, create_user, workspace, pod, issue
):
    """An expired-login failure tells the user what to do — the reason
    label and the re-authenticate action, not only the raw stderr — and
    says the automatic clock is stopped until they act."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)

    finalize_run_terminal(
        runner,
        run.id,
        AgentRunStatus.FAILED,
        error_detail="401 authentication_failed: invalid authentication credentials",
        runner_failure_reason="agent_crash",
    )

    comments = list(IssueComment.objects.filter(issue=issue))
    assert len(comments) == 1
    body = comments[0].comment_html
    assert "Provider authentication or access failed" in body
    assert "Re-authenticate" in body
    assert "Re-tick" in body
    # The raw detail stays visible for debugging.
    assert "authentication_failed" in body


@pytest.mark.unit
def test_transient_network_failure_posts_no_comment(
    db, create_user, workspace, pod, issue
):
    """A free-retry failure is being retried silently on a short backoff;
    posting it each attempt would be noise."""
    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)

    finalize_run_terminal(
        runner,
        run.id,
        AgentRunStatus.FAILED,
        error_detail="connection reset by peer",
        runner_failure_reason="agent_crash",
    )

    run.refresh_from_db()
    assert run.failure_reason == "agent_error.provider_network"
    assert IssueComment.objects.filter(issue=issue).count() == 0


@pytest.mark.unit
def test_cloud_writer_safety_net_uses_error_code(db, create_user, workspace, pod, issue):
    """Cloud-side writers (Cloud Agent tasks, reapers) go through
    ``finalize_agent_run`` with an error_code and no runner reason."""
    from pi_dash.runner.services.agent_run_finalization import finalize_agent_run

    runner = _make_runner(create_user, workspace, pod)
    run = _make_run(create_user, workspace, pod, runner, issue)

    assert finalize_agent_run(
        run.id,
        AgentRunStatus.FAILED,
        updates={"error_code": "run_timeout", "error": "Cloud Agent worker was lost or exceeded its deadline"},
    )
    run.refresh_from_db()
    assert run.failure_reason == "timeout"


@pytest.mark.unit
def test_backfill_migration_classifies_existing_failed_rows(
    db, create_user, workspace, pod, issue
):
    """The 0031 data migration runs the write-path classifier over rows
    that predate the failure_reason column."""
    from importlib import import_module

    from django.apps import apps as django_apps

    runner = _make_runner(create_user, workspace, pod)
    auth_run = _make_run(create_user, workspace, pod, runner, issue, status=AgentRunStatus.FAILED)
    stall_run = _make_run(create_user, workspace, pod, runner, issue, status=AgentRunStatus.FAILED)
    ok_run = _make_run(create_user, workspace, pod, runner, issue, status=AgentRunStatus.COMPLETED)
    AgentRun.objects.filter(pk=auth_run.pk).update(
        error="401 authentication_failed: invalid authentication credentials", failure_reason=""
    )
    AgentRun.objects.filter(pk=stall_run.pk).update(
        error="agent stalled: no events for >360s", failure_reason=""
    )

    migration = import_module("pi_dash.runner.migrations.0031_backfill_failure_reason")
    migration.backfill_failure_reason(django_apps, None)

    auth_run.refresh_from_db()
    stall_run.refresh_from_db()
    ok_run.refresh_from_db()
    assert auth_run.failure_reason == "agent_error.provider_auth_or_access"
    assert stall_run.failure_reason == "timeout"
    assert ok_run.failure_reason == ""
