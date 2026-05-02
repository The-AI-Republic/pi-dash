# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from unittest import mock

import pytest
from crum import impersonate

from pi_dash.db.models import Issue, Project, State
from pi_dash.orchestration import service
from pi_dash.prompting.seed import seed_default_template
from pi_dash.runner.models import AgentRun, AgentRunStatus


@pytest.fixture
def seeded(db):
    seed_default_template()


@pytest.fixture
def project(db, workspace, create_user):
    # ``BaseModel.save`` pulls ``created_by`` from ``crum.get_current_user()``
    # and wipes any direct FK assignment. Tests run without a request, so we
    # impersonate the user explicitly to populate the audit fields.
    with impersonate(create_user):
        return Project.objects.create(
            name="Web",
            identifier="WEB",
            workspace=workspace,
            created_by=create_user,
        )


@pytest.fixture
def states(project, create_user):
    with impersonate(create_user):
        return {
            "todo": State.objects.create(
                name="Todo", project=project, group="unstarted"
            ),
            "in_progress": State.objects.create(
                name="In Progress", project=project, group="started"
            ),
            "in_review": State.objects.create(
                name="In Review", project=project, group="review"
            ),
            "done": State.objects.create(
                name="Done", project=project, group="completed"
            ),
        }


@pytest.fixture
def issue(workspace, project, states, create_user):
    with impersonate(create_user):
        return Issue.objects.create(
            name="Task",
            workspace=workspace,
            project=project,
            state=states["todo"],
            created_by=create_user,
        )


@pytest.fixture(autouse=True)
def no_runner_dispatch(monkeypatch):
    """Every test in this file exercises orchestration logic, not the runner
    fan-out. Stub the post-commit drain so tests don't need a Redis.

    After Phase 3 of the pod design, `_dispatch_to_runner` was replaced with
    a `transaction.on_commit(drain_pod_by_id)` call inside
    `_create_and_dispatch_run`. We patch the matcher entry point and also
    force `transaction.on_commit` to fire immediately so the call is
    observable to the test.
    """
    from pi_dash.runner.services import matcher

    drain_mock = mock.Mock()
    monkeypatch.setattr(matcher, "drain_pod_by_id", drain_mock)
    monkeypatch.setattr(
        "django.db.transaction.on_commit",
        lambda fn, **kw: fn(),
    )
    return drain_mock


@pytest.mark.unit
def test_todo_to_in_progress_creates_run(seeded, issue, states):
    outcome = service.handle_issue_state_transition(
        issue=issue,
        from_state=states["todo"],
        to_state=states["in_progress"],
    )
    assert outcome.reason == "created"
    assert outcome.created_run is not None
    assert outcome.created_run.status == AgentRunStatus.QUEUED
    assert outcome.created_run.parent_run_id is None
    assert "Pi Dash issue" in outcome.created_run.prompt


@pytest.mark.unit
def test_no_op_when_active_run_exists(seeded, issue, states, workspace, create_user):
    AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="x",
        work_item=issue,
        status=AgentRunStatus.RUNNING,
    )
    outcome = service.handle_issue_state_transition(
        issue=issue,
        from_state=states["todo"],
        to_state=states["in_progress"],
    )
    assert outcome.reason == "active-run-exists"
    assert outcome.created_run is None


@pytest.mark.unit
def test_follow_up_run_links_parent(
    seeded, issue, states, workspace, create_user
):
    prior = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="first attempt",
        work_item=issue,
        status=AgentRunStatus.BLOCKED,
    )
    outcome = service.handle_issue_state_transition(
        issue=issue,
        from_state=states["todo"],
        to_state=states["in_progress"],
    )
    assert outcome.reason == "created"
    assert outcome.created_run.parent_run_id == prior.id


@pytest.mark.unit
def test_non_trigger_state_does_nothing(seeded, issue, states):
    outcome = service.handle_issue_state_transition(
        issue=issue,
        from_state=states["todo"],
        to_state=states["done"],
    )
    assert outcome.reason == "not-a-trigger-state"
    assert outcome.created_run is None


@pytest.mark.unit
def test_run_config_carries_git_fields_from_issue_and_project(
    seeded, project, issue, states
):
    project.repo_url = "git@github.com:acme/web.git"
    project.base_branch = "develop"
    project.save(update_fields=["repo_url", "base_branch"])
    issue.git_work_branch = "feat/pinned"
    issue.save(update_fields=["git_work_branch"])
    # ``BaseModel.save`` pulls ``created_by`` from ``crum.get_current_user()``;
    # tests run outside a request, so the save above wipes ``created_by``
    # in memory (DB row is unchanged because ``update_fields`` is scoped).
    # Reload so the orchestration handler can resolve a fallback creator.
    issue.refresh_from_db()
    project.refresh_from_db()

    outcome = service.handle_issue_state_transition(
        issue=issue,
        from_state=states["todo"],
        to_state=states["in_progress"],
    )
    assert outcome.reason == "created"
    cfg = outcome.created_run.run_config
    assert cfg["repo_url"] == "git@github.com:acme/web.git"
    assert cfg["repo_ref"] == "develop"
    assert cfg["git_work_branch"] == "feat/pinned"


@pytest.mark.unit
def test_run_config_empty_git_fields_surface_as_none(seeded, issue, states):
    # Project defaults to blank repo_url; issue defaults to blank work branch.
    # Both must land as ``None`` so the runner / prompt fallbacks kick in
    # rather than comparing against empty strings.
    outcome = service.handle_issue_state_transition(
        issue=issue,
        from_state=states["todo"],
        to_state=states["in_progress"],
    )
    cfg = outcome.created_run.run_config
    assert cfg["repo_url"] is None
    assert cfg["git_work_branch"] is None


# ---------------------------------------------------------------------------
# Schedule arm/disarm side effects of state transitions
# (.ai_design/issue_ticking_system/design.md §4.1, §4.4).
# ---------------------------------------------------------------------------


@pytest.fixture
def paused_state(project, create_user):
    with impersonate(create_user):
        return State.objects.create(
            name="Paused", project=project, group="backlog"
        )


@pytest.mark.unit
def test_state_transition_arms_schedule_on_in_progress_entry(
    seeded, issue, states
):
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    assert IssueAgentTicker.objects.filter(issue=issue).exists() is False
    service.handle_issue_state_transition(
        issue=issue,
        from_state=states["todo"],
        to_state=states["in_progress"],
    )
    sched = IssueAgentTicker.objects.get(issue=issue)
    assert sched.enabled is True
    assert sched.tick_count == 0
    assert sched.next_run_at is not None


@pytest.mark.unit
def test_state_transition_disarms_schedule_on_started_exit(
    seeded, issue, states, paused_state
):
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    # Seed an active schedule by transitioning into In Progress.
    service.handle_issue_state_transition(
        issue=issue,
        from_state=states["todo"],
        to_state=states["in_progress"],
    )
    assert IssueAgentTicker.objects.get(issue=issue).enabled is True

    # Now leave Started — schedule must disarm.
    service.handle_issue_state_transition(
        issue=issue,
        from_state=states["in_progress"],
        to_state=paused_state,
    )
    assert IssueAgentTicker.objects.get(issue=issue).enabled is False


@pytest.mark.unit
def test_state_transition_dispatch_immediate_false_skips_run_creation(
    seeded, issue, states
):
    """Comment & Run on Paused issue path: caller arms schedule via the
    transition but owns the dispatch separately."""
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    outcome = service.handle_issue_state_transition(
        issue=issue,
        from_state=states["todo"],
        to_state=states["in_progress"],
        dispatch_immediate=False,
    )
    assert outcome.reason == "dispatch-deferred-to-caller"
    assert outcome.created_run is None
    assert AgentRun.objects.filter(work_item=issue).count() == 0
    # Schedule still armed — the caller will dispatch its own run.
    assert IssueAgentTicker.objects.get(issue=issue).enabled is True


# ---------------------------------------------------------------------------
# Comment-triggered continuation (§5.2 of the design doc).
# ---------------------------------------------------------------------------


@pytest.fixture
def workpad_bot_user(db):
    from pi_dash.orchestration.workpad import get_agent_system_user

    return get_agent_system_user()


@pytest.fixture
def runner_for_workspace(db, workspace, project, create_user):
    from django.utils import timezone

    from pi_dash.runner.models import Connection, Pod, Runner, RunnerStatus

    pod = Pod.default_for_project(project)
    connection = Connection.objects.create(
        workspace=workspace,
        created_by=create_user,
        name="connection_agentA",
        secret_hash="sh-agentA",
        secret_fingerprint="sf-agentA",
        enrolled_at=timezone.now(),
    )
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        connection=connection,
        name="agentA",
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


def _make_comment(issue, actor, body="please continue"):
    from pi_dash.db.models.issue import IssueComment

    with impersonate(actor):
        # IssueComment derives comment_stripped from comment_html on save,
        # so we must populate the HTML field for the body to survive.
        return IssueComment.objects.create(
            issue=issue,
            project=issue.project,
            workspace=issue.workspace,
            actor=actor,
            comment_html=f"<p>{body}</p>",
        )


def _make_paused_run(issue, runner, *, thread_id="sess_xyz"):
    from django.utils import timezone

    from pi_dash.runner.models import AgentRun, AgentRunStatus

    return AgentRun.objects.create(
        workspace=issue.workspace,
        # AgentRun.save mirrors owner → created_by; anchor on runner.owner
        # rather than issue.created_by (which a non-impersonated state save
        # may have cleared).
        owner=runner.owner,
        pod=runner.pod,
        work_item=issue,
        runner=runner,
        thread_id=thread_id,
        status=AgentRunStatus.PAUSED_AWAITING_INPUT,
        prompt="prior work",
        started_at=timezone.now() - timezone.timedelta(minutes=5),
    )


@pytest.mark.unit
def test_comment_creates_pinned_continuation(
    seeded, project, issue, states, runner_for_workspace, create_user
):
    """``handle_issue_comment`` is the explicit entry point Comment & Run
    invokes — comments themselves no longer fire it via post_save."""
    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    prior = _make_paused_run(issue, runner_for_workspace)

    comment = _make_comment(issue, create_user, "use option B please")
    outcome = service.handle_issue_comment(comment)
    assert outcome.reason == "created"

    r_next = (
        AgentRun.objects.filter(work_item=issue, parent_run=prior)
        .order_by("-created_at")
        .first()
    )
    assert r_next is not None
    assert r_next.pinned_runner_id == runner_for_workspace.id
    assert r_next.status == AgentRunStatus.QUEUED
    assert "option B" in r_next.prompt


@pytest.mark.unit
def test_comment_from_bot_is_ignored(
    seeded, project, issue, states, runner_for_workspace, workpad_bot_user
):
    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    _make_paused_run(issue, runner_for_workspace)

    comment = _make_comment(issue, workpad_bot_user, "## Agent Workpad\n...")
    outcome = service.handle_issue_comment(comment)
    assert outcome.reason == "bot-comment"
    assert outcome.created_run is None


@pytest.mark.unit
def test_comment_on_backlog_issue_is_ignored(
    seeded, project, issue, states, runner_for_workspace, create_user
):
    # Issue.state defaults to states["todo"] (group=unstarted) — backlog-side.
    _make_paused_run(issue, runner_for_workspace)
    comment = _make_comment(issue, create_user)
    outcome = service.handle_issue_comment(comment)
    assert outcome.reason == "state-not-eligible"


@pytest.mark.unit
def test_comment_with_no_prior_run_skipped(
    seeded, project, issue, states, create_user
):
    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    comment = _make_comment(issue, create_user)
    outcome = service.handle_issue_comment(comment)
    assert outcome.reason == "no-prior-run"


@pytest.mark.unit
def test_comment_during_active_run_returns_prior_run_active(
    seeded, project, issue, states, runner_for_workspace, create_user
):
    """Even when called explicitly, ``handle_issue_comment`` skips when a
    prior run is in flight — Comment & Run on a busy issue is a no-op."""
    from django.utils import timezone

    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=create_user,
        pod=runner_for_workspace.pod,
        work_item=issue,
        runner=runner_for_workspace,
        status=AgentRunStatus.RUNNING,
        prompt="working",
        started_at=timezone.now() - timezone.timedelta(minutes=1),
    )
    comment = _make_comment(issue, create_user, "fyi")
    outcome = service.handle_issue_comment(comment)
    assert outcome.reason == "prior-run-active"
    assert outcome.created_run is None


@pytest.mark.unit
def test_pin_skipped_when_parent_has_no_thread_id(
    seeded, project, issue, states, runner_for_workspace, create_user
):
    """No thread_id means there's no session to resume against; don't pin."""
    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    prior = _make_paused_run(issue, runner_for_workspace, thread_id="")
    assert prior.thread_id == ""

    comment = _make_comment(issue, create_user, "go")
    outcome = service.handle_issue_comment(comment)
    assert outcome.reason == "created"
    r_next = (
        AgentRun.objects.filter(work_item=issue, parent_run=prior)
        .order_by("-created_at")
        .first()
    )
    assert r_next is not None
    assert r_next.pinned_runner_id is None


# ---------------------------------------------------------------------------
# Re-arm on human comment (.ai_design/create_review_state/design.md §4.6)
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_comment_rearms_terminally_disarmed_ticker(
    seeded, project, issue, states, runner_for_workspace, create_user
):
    """A follow-up human comment after a `completed` terminal disarm
    must flip ``enabled=True`` and clear ``disarm_reason``."""
    from pi_dash.db.models.issue_agent_ticker import (
        IssueAgentTicker,
        TickerDisarmReason,
    )
    from pi_dash.orchestration import scheduling

    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    _make_paused_run(issue, runner_for_workspace)
    scheduling.arm_ticker(issue)
    sched = IssueAgentTicker.objects.get(issue=issue)
    sched.enabled = False
    sched.disarm_reason = TickerDisarmReason.TERMINAL_SIGNAL
    sched.tick_count = 5
    sched.save(update_fields=["enabled", "disarm_reason", "tick_count"])

    comment = _make_comment(issue, create_user, "I disagree, look again")
    service.handle_issue_comment(comment)

    sched.refresh_from_db()
    assert sched.enabled is True
    assert sched.disarm_reason == TickerDisarmReason.NONE
    assert sched.tick_count == 0


@pytest.mark.unit
def test_comment_rearms_even_when_continuation_coalesces(
    seeded, project, issue, states, runner_for_workspace, create_user
):
    """Re-arm fires before the coalesce / active-run / no-pod returns
    so ticking restarts even when no new run dispatches."""
    from django.utils import timezone

    from pi_dash.db.models.issue_agent_ticker import (
        IssueAgentTicker,
        TickerDisarmReason,
    )
    from pi_dash.orchestration import scheduling

    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    prior = _make_paused_run(issue, runner_for_workspace)
    AgentRun.objects.create(
        workspace=issue.workspace,
        owner=runner_for_workspace.owner,
        pod=runner_for_workspace.pod,
        work_item=issue,
        parent_run=prior,
        status=AgentRunStatus.QUEUED,
        prompt="already queued",
        started_at=timezone.now(),
    )
    scheduling.arm_ticker(issue)
    sched = IssueAgentTicker.objects.get(issue=issue)
    sched.enabled = False
    sched.disarm_reason = TickerDisarmReason.TERMINAL_SIGNAL
    sched.save(update_fields=["enabled", "disarm_reason"])

    comment = _make_comment(issue, create_user, "ping")
    outcome = service.handle_issue_comment(comment)
    assert outcome.reason == "coalesced"
    sched.refresh_from_db()
    assert sched.enabled is True


@pytest.mark.unit
def test_comment_does_not_rearm_user_disabled_ticker(
    seeded, project, issue, states, runner_for_workspace, create_user
):
    """The user_disabled escape hatch survives re-arm-on-comment."""
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker
    from pi_dash.orchestration import scheduling

    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    _make_paused_run(issue, runner_for_workspace)
    scheduling.arm_ticker(issue)
    sched = IssueAgentTicker.objects.get(issue=issue)
    sched.user_disabled = True
    sched.enabled = False
    sched.save(update_fields=["user_disabled", "enabled"])

    comment = _make_comment(issue, create_user, "wake up")
    service.handle_issue_comment(comment)

    sched.refresh_from_db()
    assert sched.user_disabled is True
    assert sched.enabled is False


@pytest.mark.unit
def test_bot_comment_does_not_rearm(
    seeded, project, issue, states, runner_for_workspace, workpad_bot_user
):
    """Bot comments early-return before re-arm; no ticker mutation."""
    from pi_dash.db.models.issue_agent_ticker import (
        IssueAgentTicker,
        TickerDisarmReason,
    )
    from pi_dash.orchestration import scheduling

    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    _make_paused_run(issue, runner_for_workspace)
    scheduling.arm_ticker(issue)
    sched = IssueAgentTicker.objects.get(issue=issue)
    sched.enabled = False
    sched.disarm_reason = TickerDisarmReason.TERMINAL_SIGNAL
    sched.save(update_fields=["enabled", "disarm_reason"])

    comment = _make_comment(issue, workpad_bot_user, "## Workpad")
    service.handle_issue_comment(comment)

    sched.refresh_from_db()
    assert sched.enabled is False
    assert sched.disarm_reason == TickerDisarmReason.TERMINAL_SIGNAL


@pytest.mark.unit
def test_comment_on_non_eligible_state_does_not_rearm(
    seeded, project, issue, states, runner_for_workspace, create_user
):
    """Comment on a Done/Backlog/Cancelled issue does not re-arm —
    the early ``state-not-eligible`` return precedes the re-arm call.
    """
    from pi_dash.db.models.issue_agent_ticker import (
        IssueAgentTicker,
        TickerDisarmReason,
    )
    from pi_dash.orchestration import scheduling

    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    _make_paused_run(issue, runner_for_workspace)
    scheduling.arm_ticker(issue)
    sched = IssueAgentTicker.objects.get(issue=issue)
    sched.enabled = False
    sched.disarm_reason = TickerDisarmReason.TERMINAL_SIGNAL
    sched.save(update_fields=["enabled", "disarm_reason"])

    Issue.all_objects.filter(pk=issue.pk).update(state=states["done"])
    issue.refresh_from_db()

    comment = _make_comment(issue, create_user, "ping")
    outcome = service.handle_issue_comment(comment)
    assert outcome.reason == "state-not-eligible"
    sched.refresh_from_db()
    assert sched.enabled is False


# ---------------------------------------------------------------------------
# In Review phase + cross-phase fresh session
# (.ai_design/create_review_state/design.md §4.2 / §4.3)
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_is_delegation_trigger_true_for_in_review(states):
    assert service._is_delegation_trigger(states["in_review"]) is True


@pytest.mark.unit
def test_in_progress_to_in_review_dispatches_fresh_session(
    seeded, issue, states, runner_for_workspace
):
    """Cross-phase entry into review must dispatch with parent_run=None
    and pinned_runner cleared so the ``review`` template body lands as
    the system prompt of a fresh agent session."""
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    prior = _make_paused_run(issue, runner_for_workspace)

    outcome = service.handle_issue_state_transition(
        issue=issue,
        from_state=states["in_progress"],
        to_state=states["in_review"],
    )
    assert outcome.reason == "created"
    assert outcome.created_run is not None
    assert outcome.created_run.parent_run_id is None
    assert outcome.created_run.pinned_runner_id is None
    # ``resume_parent_run`` must capture the latest impl run so the
    # reverse hand-back can resume it.
    sched = IssueAgentTicker.objects.get(issue=issue)
    assert sched.resume_parent_run_id == prior.pk


@pytest.mark.unit
def test_in_review_to_in_progress_resumes_pre_review_run(
    seeded, issue, states, runner_for_workspace
):
    """Hand-back transition uses ``resume_parent_run`` (the impl run we
    stashed on entry to review) as parent — NOT the latest review run.
    """
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_progress"])
    issue.refresh_from_db()
    impl_run = _make_paused_run(issue, runner_for_workspace, thread_id="impl")

    # Transition to In Review, which captures impl_run on the ticker.
    service.handle_issue_state_transition(
        issue=issue,
        from_state=states["in_progress"],
        to_state=states["in_review"],
    )
    # Sanity: a review run exists and is the latest prior.
    review_run = (
        AgentRun.objects.filter(work_item=issue)
        .order_by("-created_at")
        .first()
    )
    assert review_run is not None
    assert review_run.id != impl_run.id

    # Now hand back to In Progress.
    outcome = service.handle_issue_state_transition(
        issue=issue,
        from_state=states["in_review"],
        to_state=states["in_progress"],
    )
    assert outcome.reason == "created"
    # Parent should be the impl_run, NOT the latest review run.
    assert outcome.created_run.parent_run_id == impl_run.id


@pytest.mark.unit
def test_continuation_eligible_groups_includes_review():
    assert "review" in service.CONTINUATION_ELIGIBLE_GROUPS
    assert "started" in service.CONTINUATION_ELIGIBLE_GROUPS


@pytest.mark.unit
def test_comment_on_in_review_wakes_agent(
    seeded, project, issue, states, runner_for_workspace, create_user
):
    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_review"])
    issue.refresh_from_db()
    _make_paused_run(issue, runner_for_workspace)

    comment = _make_comment(issue, create_user, "double-check finding #2")
    outcome = service.handle_issue_comment(comment)
    assert outcome.reason == "created"


@pytest.mark.unit
def test_state_transition_disarms_on_leaving_review(
    seeded, issue, states
):
    """Review group exits also disarm the ticker (generalized rule)."""
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    Issue.all_objects.filter(pk=issue.pk).update(state=states["in_review"])
    issue.refresh_from_db()
    # Seed a ticker on the issue.
    from pi_dash.orchestration import scheduling

    scheduling.arm_ticker(issue)
    assert IssueAgentTicker.objects.get(issue=issue).enabled is True

    service.handle_issue_state_transition(
        issue=issue,
        from_state=states["in_review"],
        to_state=states["done"],
    )
    assert IssueAgentTicker.objects.get(issue=issue).enabled is False
