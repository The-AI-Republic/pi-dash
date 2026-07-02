# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the runner-eligibility preflight + bounce-to-Backlog behavior.

See ``.ai_design/issue_runner/design.md`` §6.6.
"""

from __future__ import annotations

from unittest import mock
from uuid import uuid4

import pytest
from crum import impersonate
from django.utils import timezone

from pi_dash.db.models import Issue, Project, State
from pi_dash.db.models.issue import IssueAssignee, IssueComment
from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker
from pi_dash.db.models.user import User
from pi_dash.db.models.workspace import WorkspaceMember
from pi_dash.orchestration import scheduling, service as orchestration_service
from pi_dash.orchestration.workpad import get_agent_system_user
from pi_dash.runner.models import (
    AgentRun,
    AgentRunTrigger,
    Pod,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services import matcher


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def project(db, workspace, create_user):
    with impersonate(create_user):
        return Project.objects.create(
            name="Web",
            identifier="WEB",
            workspace=workspace,
            created_by=create_user,
        )


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def states(project, create_user):
    """Project states. Two Backlog states so the 'prefers default' rule
    can be checked. Includes In Progress so we can transition issues into
    a ticking state for the integration tests."""
    with impersonate(create_user):
        return {
            "backlog_default": State.objects.create(
                name="Backlog",
                project=project,
                group="backlog",
                default=True,
                sequence=100,
            ),
            "backlog_other": State.objects.create(
                name="Icebox",
                project=project,
                group="backlog",
                default=False,
                sequence=200,
            ),
            "todo": State.objects.create(
                name="Todo", project=project, group="unstarted"
            ),
            "in_progress": State.objects.create(
                name="In Progress", project=project, group="started"
            ),
        }


def _make_user(email_suffix: str) -> User:
    user = User.objects.create(
        email=f"{email_suffix}@example.com",
        username=f"user_{email_suffix}_{uuid4().hex[:6]}",
        first_name=email_suffix.capitalize(),
        last_name="Tester",
    )
    user.set_password("pw")
    user.save()
    return user


@pytest.fixture
def user_a(create_user):
    """The default user from the conftest workspace fixture; the workspace
    admin / issue creator in most scenarios."""
    return create_user


@pytest.fixture
def user_b(db, workspace):
    u = _make_user("b")
    WorkspaceMember.objects.create(workspace=workspace, member=u, role=15)
    return u


@pytest.fixture
def user_c(db, workspace):
    u = _make_user("c")
    WorkspaceMember.objects.create(workspace=workspace, member=u, role=15)
    return u


def _make_runner(owner, workspace, pod, name="rnr", status=RunnerStatus.ONLINE):
    return Runner.objects.create(
        owner=owner,
        workspace=workspace,
        pod=pod,
        name=name,
        status=status,
        last_heartbeat_at=(
            timezone.now() if status == RunnerStatus.ONLINE else None
        ),
    )


@pytest.fixture
def issue(workspace, project, states, user_a):
    """Issue created in Backlog by user_a so the state-transition signal
    does not auto-fire on creation."""
    with impersonate(user_a):
        return Issue.objects.create(
            name="Task",
            workspace=workspace,
            project=project,
            state=states["backlog_default"],
            created_by=user_a,
        )


@pytest.fixture(autouse=True)
def _on_commit_immediate(monkeypatch):
    """Pytest-django wraps each test in a rolled-back transaction so
    on_commit callbacks never fire. Run them inline so dispatch
    side-effects (drain, ticker arming) are observable."""
    monkeypatch.setattr(
        "django.db.transaction.on_commit", lambda fn, **kw: fn()
    )


@pytest.fixture(autouse=True)
def _stub_drain(monkeypatch):
    """The matcher's drain isn't under test here; suppress it so dispatch
    paths don't try to dispatch over a (mocked) WebSocket."""
    monkeypatch.setattr(
        matcher, "drain_pod_by_id", mock.Mock()
    )
    monkeypatch.setattr(
        matcher, "drain_pod", mock.Mock()
    )


# ---------------------------------------------------------------------------
# matcher.pod_has_runner_for_issue_principal
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_pod_has_runner_matches_run_creator(db, pod, issue, user_a):
    _make_runner(user_a, issue.workspace, pod)
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, user_a.id) is True


@pytest.mark.unit
def test_pod_has_runner_matches_issue_creator(db, pod, issue, user_a, user_b):
    # Run is created by user_b (e.g., via Comment & Run), but issue.created_by
    # is user_a. user_a's runner should make the pod eligible.
    _make_runner(user_a, issue.workspace, pod)
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, user_b.id) is True


@pytest.mark.unit
def test_pod_has_runner_matches_assignee(db, pod, issue, user_a, user_b, user_c):
    # Only user_c has a runner. user_c is assigned. Run creator is user_a,
    # issue creator is user_a — neither has a runner. The assignee path
    # should still pass.
    _make_runner(user_c, issue.workspace, pod)
    IssueAssignee.objects.create(
        issue=issue, assignee=user_c, project=issue.project
    )
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, user_a.id) is True


@pytest.mark.unit
def test_pod_has_runner_no_eligible_owner(db, pod, issue, user_a, user_b):
    # Only user_b has a runner; user_b is NOT the creator, issue.created_by,
    # or an assignee.
    _make_runner(user_b, issue.workspace, pod)
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, user_a.id) is False


@pytest.mark.unit
def test_pod_has_runner_revoked_runner_does_not_count(db, pod, issue, user_a):
    """REVOKED is permanent — a revoked runner can never pick the run up
    (``drain_pod`` assigns ONLINE only), so counting it would let the run be
    created and jam in QUEUED forever. It must NOT satisfy the predicate
    (consistent with ``count_active`` / ``can_register_another``)."""
    _make_runner(user_a, issue.workspace, pod, status=RunnerStatus.REVOKED)
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, user_a.id) is False


@pytest.mark.unit
def test_pod_has_runner_offline_runner_still_counts(db, pod, issue, user_a):
    _make_runner(user_a, issue.workspace, pod, status=RunnerStatus.OFFLINE)
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, user_a.id) is True


@pytest.mark.unit
def test_pod_has_runner_revoked_ignored_but_live_owner_still_counts(
    db, pod, issue, user_a
):
    """A REVOKED runner doesn't count, but a second ONLINE runner under the
    same eligible owner still makes the pod eligible."""
    _make_runner(user_a, issue.workspace, pod, name="dead", status=RunnerStatus.REVOKED)
    _make_runner(user_a, issue.workspace, pod, name="live", status=RunnerStatus.ONLINE)
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, user_a.id) is True


@pytest.mark.unit
def test_dispatch_run_ai_run_bounces_when_only_runner_revoked(
    db, issue, pod, user_a, states
):
    """Regression for the silent-QUEUED jam: the eligible owner's *only*
    runner is REVOKED (permanent — ``drain_pod`` assigns ONLINE only). The
    preflight must bounce rather than create an AgentRun that would jam in
    QUEUED forever."""
    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    _make_runner(user_a, issue.workspace, pod, status=RunnerStatus.REVOKED)

    run = scheduling.dispatch_run_ai_run(issue, actor=user_a)

    assert run is None
    assert AgentRun.objects.filter(work_item=issue).count() == 0
    issue.refresh_from_db()
    assert issue.state.group == "backlog"
    assert IssueComment.objects.filter(issue=issue).count() == 1


@pytest.mark.unit
def test_pod_has_runner_empty_pod(db, pod, issue, user_a):
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, user_a.id) is False


@pytest.mark.unit
def test_pod_has_runner_other_pod_does_not_count(
    db, project, pod, issue, user_a, create_user
):
    # A runner owned by user_a exists, but in a different pod (different
    # project) — must not satisfy the predicate.
    other_project = Project.objects.create(
        name="Other",
        identifier="OTH",
        workspace=issue.workspace,
        created_by=create_user,
    )
    other_pod = Pod.default_for_project(other_project)
    _make_runner(user_a, issue.workspace, other_pod, name="elsewhere")
    assert matcher.pod_has_runner_for_issue_principal(pod, issue, user_a.id) is False


# ---------------------------------------------------------------------------
# scheduling.preflight_eligibility_or_bounce — happy path
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_preflight_returns_true_when_eligible(db, issue, pod, user_a):
    _make_runner(user_a, issue.workspace, pod)
    assert (
        scheduling.preflight_eligibility_or_bounce(
            issue,
            run_creator=user_a,
            pod=pod,
            triggered_by=AgentRunTrigger.STATE_TRANSITION.value,
        )
        is True
    )
    # No bounce side-effects.
    assert IssueComment.objects.filter(issue=issue).count() == 0
    issue.refresh_from_db()
    assert issue.state.group == "backlog"  # unchanged (already in backlog)


# ---------------------------------------------------------------------------
# scheduling.preflight_eligibility_or_bounce — bounce path
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_preflight_bounces_when_no_eligible_runner(
    db, issue, pod, user_a, user_b, states
):
    """B's runner exists, but A creates run on A's issue — bounce."""
    # Put the issue in In Progress so we can verify the state move back.
    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()

    _make_runner(user_b, issue.workspace, pod)
    result = scheduling.preflight_eligibility_or_bounce(
        issue,
        run_creator=user_a,
        pod=pod,
        triggered_by=AgentRunTrigger.STATE_TRANSITION.value,
    )
    assert result is False

    issue.refresh_from_db()
    assert issue.state.group == "backlog"
    assert issue.state.name == "Backlog"  # default backlog state, not "Icebox"

    comment = IssueComment.objects.get(issue=issue)
    assert comment.actor == get_agent_system_user()
    assert comment.speaker_type == IssueComment.SpeakerType.AGENT
    assert "no eligible runner" in comment.comment_html.lower()


@pytest.mark.unit
def test_bounce_prefers_default_backlog_state(
    db, issue, pod, user_a, user_b, states, project
):
    """When project has two Backlog states, the one with default=True wins."""
    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()

    _make_runner(user_b, issue.workspace, pod)
    scheduling.preflight_eligibility_or_bounce(
        issue,
        run_creator=user_a,
        pod=pod,
        triggered_by=AgentRunTrigger.RUN_AI.value,
    )

    issue.refresh_from_db()
    assert issue.state == states["backlog_default"]
    assert issue.state != states["backlog_other"]


@pytest.mark.unit
def test_bounce_skips_state_move_when_already_in_backlog(
    db, issue, pod, user_a, user_b
):
    """Issue is already Backlog. State must not change, but the comment
    explaining why dispatch was skipped still posts."""
    original_state_id = issue.state_id
    _make_runner(user_b, issue.workspace, pod)

    result = scheduling.preflight_eligibility_or_bounce(
        issue,
        run_creator=user_a,
        pod=pod,
        triggered_by=AgentRunTrigger.RUN_AI.value,
    )
    assert result is False

    issue.refresh_from_db()
    assert issue.state_id == original_state_id
    assert IssueComment.objects.filter(issue=issue).count() == 1


@pytest.mark.unit
def test_bounce_falls_back_gracefully_when_no_backlog_state(
    db, workspace, create_user, user_a, user_b
):
    """Defensive: project has no Backlog state and no non-ticking fallback.
    The bounce must not crash, must still post the comment, and must
    **disarm the ticker** — otherwise the issue stays in a ticking state and
    the next tick re-enters the bounce, spamming a comment each time."""
    with impersonate(create_user):
        bare_project = Project.objects.create(
            name="Bare",
            identifier="BARE",
            workspace=workspace,
            created_by=create_user,
        )
    pod = Pod.default_for_project(bare_project)
    # Neutral (non-ticking) state to create the issue in, so issue creation
    # doesn't itself fire the state-transition dispatch. There is
    # deliberately no Backlog state and no default_state fallback.
    unstarted = State.objects.create(
        name="Todo", project=bare_project, group="unstarted"
    )
    started = State.objects.create(
        name="In Progress", project=bare_project, group="started"
    )
    with impersonate(user_a):
        bare_issue = Issue.objects.create(
            name="No-backlog",
            workspace=workspace,
            project=bare_project,
            state=unstarted,
            created_by=user_a,
        )
    # Move into the ticking state without firing the signal, and arm the
    # ticker as if the issue were actively running.
    Issue.all_objects.filter(pk=bare_issue.pk).update(state=started)
    bare_issue.refresh_from_db()
    IssueAgentTicker.objects.create(issue=bare_issue, enabled=True, tick_count=1)

    _make_runner(user_b, workspace, pod)

    result = scheduling.preflight_eligibility_or_bounce(
        bare_issue,
        run_creator=user_a,
        pod=pod,
        triggered_by=AgentRunTrigger.TICK.value,
    )
    assert result is False

    bare_issue.refresh_from_db()
    assert bare_issue.state_id == started.id  # unchanged — no backlog target
    assert IssueComment.objects.filter(issue=bare_issue).count() == 1
    # The loop-breaker: ticker disarmed even though the issue couldn't move.
    assert IssueAgentTicker.objects.get(issue=bare_issue).enabled is False


# ---------------------------------------------------------------------------
# Integration: dispatch entry points are gated
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_dispatch_run_ai_run_bounces_when_no_runner(
    db, issue, pod, user_a, user_b, states
):
    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    _make_runner(user_b, issue.workspace, pod)  # not eligible for user_a's issue

    run = scheduling.dispatch_run_ai_run(issue, actor=user_a)

    assert run is None
    assert AgentRun.objects.filter(work_item=issue).count() == 0
    issue.refresh_from_db()
    assert issue.state.group == "backlog"
    assert IssueComment.objects.filter(issue=issue).count() == 1


@pytest.mark.unit
def test_dispatch_run_ai_run_proceeds_when_eligible(
    db, issue, pod, user_a, states, monkeypatch
):
    """With an eligible runner present, no bounce; the dispatch proceeds.

    We stub ``build_first_turn`` so the test stays focused on the
    preflight wiring (otherwise this would silently couple to the
    prompting / templating subsystem).
    """
    monkeypatch.setattr(
        "pi_dash.orchestration.service.build_first_turn",
        lambda issue, run: "stub-prompt",
    )
    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    _make_runner(user_a, issue.workspace, pod)

    run = scheduling.dispatch_run_ai_run(issue, actor=user_a)
    assert run is not None
    assert AgentRun.objects.filter(work_item=issue).count() == 1
    issue.refresh_from_db()
    assert issue.state.group == "started"  # NOT bounced
    assert IssueComment.objects.filter(issue=issue).count() == 0


@pytest.mark.unit
def test_dispatch_continuation_run_comment_and_run_bounces(
    db, issue, pod, user_a, user_b, states
):
    """Comment & Run uses an explicit user actor (the commenter), not
    the agent bot. The preflight still bounces when neither the actor,
    the issue creator, nor any assignee owns a runner in the pod."""
    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    _make_runner(user_b, issue.workspace, pod)  # B has a runner, A doesn't

    # Prior run so dispatch_continuation_run finds a parent.
    AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=user_a,
        pod=pod,
        work_item=issue,
        status="completed",
        prompt="prior",
    )

    # user_a (issue creator) clicks Comment & Run — actor is a real user
    # here, distinguishing this from the tick path. Eligibility set is
    # {user_a.id} (creator + issue.created_by) — user_b's runner doesn't
    # match.
    run = scheduling.dispatch_continuation_run(
        issue,
        triggered_by=scheduling.TRIGGER_COMMENT_AND_RUN,
        actor=user_a,
    )
    assert run is None
    issue.refresh_from_db()
    assert issue.state.group == "backlog"
    # Both the bounce comment AND the issue creator's prior comment
    # context aren't here — only the bounce comment.
    assert IssueComment.objects.filter(issue=issue).count() == 1


@pytest.mark.unit
def test_dispatch_continuation_run_tick_bounces_when_no_runner(
    db, issue, pod, user_a, user_b, states
):
    """Tick path: created_by = agent_system_user (the bot, no runner of
    its own), so eligibility falls to issue.created_by + assignees.
    Neither matches B's runner — bounce."""
    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    _make_runner(user_b, issue.workspace, pod)

    # Continuation needs a prior run to derive the parent.
    parent = AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=user_a,
        pod=pod,
        work_item=issue,
        status="completed",
        prompt="prior",
    )
    assert parent is not None

    run = scheduling.dispatch_continuation_run(
        issue, triggered_by=scheduling.TRIGGER_TICK
    )
    assert run is None
    # The parent still exists; only the new continuation was skipped.
    assert AgentRun.objects.filter(work_item=issue, status="queued").count() == 0
    issue.refresh_from_db()
    assert issue.state.group == "backlog"


@pytest.mark.unit
def test_state_transition_to_in_progress_bounces_when_no_runner(
    db, issue, pod, user_a, user_b, states
):
    """Full integration through ``handle_issue_state_transition``: A moves
    issue Todo → In Progress, only B has a runner → end state is Backlog
    with the comment posted, no AgentRun created."""
    Issue.all_objects.filter(pk=issue.pk).update(state=states["todo"])
    issue.refresh_from_db()
    _make_runner(user_b, issue.workspace, pod)

    outcome = orchestration_service.handle_issue_state_transition(
        issue=issue,
        from_state=states["todo"],
        to_state=states["in_progress"],
        actor=user_a,
    )
    assert outcome.created_run is None
    assert outcome.reason == "no-eligible-runner"
    assert AgentRun.objects.filter(work_item=issue).count() == 0

    issue.refresh_from_db()
    assert issue.state.group == "backlog"
    assert IssueComment.objects.filter(issue=issue).count() == 1


@pytest.mark.unit
def test_bounce_disarms_ticker(db, issue, pod, user_a, user_b, states):
    """The recursive issue.save() inside the bounce fires the state
    transition signal, which disarms the ticker as a side-effect.

    Test sequence: arm the ticker manually (simulating an already-running
    issue), then run the preflight bounce, then check the ticker row has
    enabled=False."""
    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    IssueAgentTicker.objects.create(
        issue=issue, enabled=True, tick_count=3
    )
    _make_runner(user_b, issue.workspace, pod)

    scheduling.preflight_eligibility_or_bounce(
        issue,
        run_creator=user_a,
        pod=pod,
        triggered_by=AgentRunTrigger.TICK.value,
    )

    ticker = IssueAgentTicker.objects.get(issue=issue)
    assert ticker.enabled is False
