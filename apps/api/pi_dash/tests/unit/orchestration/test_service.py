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
# Comment-triggered continuation (§5.2 of the design doc).
# ---------------------------------------------------------------------------


@pytest.fixture
def workpad_bot_user(db):
    from pi_dash.orchestration.workpad import get_agent_system_user

    return get_agent_system_user()


@pytest.fixture
def runner_for_workspace(db, workspace, create_user):
    from django.utils import timezone

    from pi_dash.runner.models import Pod, Runner, RunnerStatus

    pod = Pod.default_for_workspace(workspace)
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="agentA",
        credential_hash="h",
        credential_fingerprint="f" * 12,
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
