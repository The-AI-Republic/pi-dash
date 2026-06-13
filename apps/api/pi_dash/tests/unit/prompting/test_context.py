# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.db.models import Issue, Project, State
from pi_dash.prompting.context import build_context
from pi_dash.runner.models import AgentRun


@pytest.fixture
def project(db, workspace, create_user):
    return Project.objects.create(
        name="Test Project",
        identifier="TP",
        workspace=workspace,
        created_by=create_user,
        repo_url="git@github.com:acme/web.git",
        base_branch="trunk",
    )


@pytest.fixture
def state(project):
    # Use a non-trigger state so creating the issue doesn't also fire the
    # orchestration signal hook (which would try to render a prompt before the
    # seed fixture has run).
    return State.objects.create(name="Todo", project=project, group="unstarted")


@pytest.fixture
def issue(workspace, project, state, create_user):
    return Issue.objects.create(
        name="Make button blue",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
        priority="high",
    )


@pytest.fixture
def run(db, workspace, create_user, issue):
    return AgentRun.objects.create(
        owner=create_user, workspace=workspace, prompt="", work_item=issue
    )


@pytest.mark.unit
def test_context_shape(issue, run):
    ctx = build_context(issue, run)
    assert ctx["issue"]["title"] == issue.name
    assert ctx["issue"]["priority"] == "high"
    assert ctx["issue"]["state"] == "Todo"
    assert ctx["issue"]["state_group"] == "unstarted"
    assert ctx["issue"]["identifier"].startswith("TP-")
    # run.kind is the base-context contract key shared sections branch on.
    # A Todo (unstarted) issue falls back to the default coding-task kind.
    assert ctx["run"]["kind"] == "coding-task"
    assert ctx["project"]["identifier"] == "TP"
    assert ctx["repo"]["url"] == "git@github.com:acme/web.git"
    assert ctx["repo"]["base_branch"] == "trunk"
    # No git_work_branch set on the issue → should surface as None so templates
    # can branch on `{% if repo.work_branch %}` without false positives.
    assert ctx["repo"]["work_branch"] is None
    assert ctx["run"]["attempt"] == 1
    assert ctx["run"]["turn_number"] == 1


@pytest.mark.unit
def test_context_attempt_increments_on_follow_up(
    issue, run, workspace, create_user
):
    AgentRun.objects.create(
        owner=create_user, workspace=workspace, prompt="prior", work_item=issue
    )
    ctx = build_context(issue, run)
    assert ctx["run"]["attempt"] == 2


@pytest.mark.unit
def test_context_includes_git_work_branch_when_set(issue, run):
    issue.git_work_branch = "feat/pinned-branch"
    issue.save(update_fields=["git_work_branch"])
    ctx = build_context(issue, run)
    assert ctx["repo"]["work_branch"] == "feat/pinned-branch"


@pytest.mark.unit
def test_context_parent_is_none_when_unset(issue, run):
    ctx = build_context(issue, run)
    assert ctx["parent"] is None


@pytest.mark.unit
def test_context_parent_uses_parents_own_project_identifier(
    workspace, project, state, create_user, run, issue
):
    # Parents may live in a different project than their child (the FK is just
    # a self-reference with no same-project constraint). The rendered parent
    # identifier must use the *parent's* project identifier, not the child's.
    other_project = Project.objects.create(
        name="Other Project",
        identifier="OP",
        workspace=workspace,
        created_by=create_user,
        repo_url="git@github.com:acme/other.git",
        base_branch="trunk",
    )
    other_state = State.objects.create(
        name="Todo", project=other_project, group="unstarted"
    )
    parent = Issue.objects.create(
        name="Umbrella epic",
        workspace=workspace,
        project=other_project,
        state=other_state,
        created_by=create_user,
        git_work_branch="pi-dash/op-1",
    )
    issue.parent = parent
    issue.save(update_fields=["parent"])

    ctx = build_context(issue, run)
    assert ctx["parent"] is not None
    assert ctx["parent"]["identifier"].startswith("OP-"), (
        f"parent identifier should use parent's project (OP), got "
        f"{ctx['parent']['identifier']!r}"
    )
    assert ctx["parent"]["title"] == "Umbrella epic"
    assert ctx["parent"]["work_branch"] == "pi-dash/op-1"


@pytest.mark.unit
def test_context_parent_work_branch_empty_surfaces_as_none(
    workspace, project, state, create_user, run, issue
):
    parent = Issue.objects.create(
        name="Sibling parent",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
        git_work_branch="",
    )
    issue.parent = parent
    issue.save(update_fields=["parent"])

    ctx = build_context(issue, run)
    assert ctx["parent"] is not None
    assert ctx["parent"]["work_branch"] is None


@pytest.mark.unit
def test_context_parent_includes_description_and_comment_count(
    workspace, project, state, create_user, run, issue
):
    from pi_dash.db.models import IssueComment

    parent = Issue.objects.create(
        name="Umbrella epic",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
        description_html="<p>Parent framing and acceptance criteria.</p>",
    )
    for body in ("<p>first</p>", "<p>second</p>"):
        IssueComment.objects.create(
            issue=parent,
            workspace=workspace,
            project=project,
            created_by=create_user,
            comment_html=body,
        )
    issue.parent = parent
    issue.save(update_fields=["parent"])

    ctx = build_context(issue, run)
    assert ctx["parent"]["description"] == "Parent framing and acceptance criteria."
    # Comment count surfaces the discussion volume without inlining bodies.
    assert ctx["parent"]["comments_count"] == 2


@pytest.mark.unit
def test_context_lineage_is_none_for_single_parent(
    workspace, project, state, create_user, run, issue
):
    # A direct parent with no ancestors → the `parent` block carries
    # everything, so no separate lineage tree is emitted.
    parent = Issue.objects.create(
        name="Lone parent",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
    )
    issue.parent = parent
    issue.save(update_fields=["parent"])

    ctx = build_context(issue, run)
    assert ctx["parent"] is not None
    assert ctx["lineage"] is None


@pytest.mark.unit
def test_context_lineage_populated_for_grandparent(
    workspace, project, state, create_user, run, issue
):
    grandparent = Issue.objects.create(
        name="Root epic",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
    )
    parent = Issue.objects.create(
        name="Mid epic",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
        parent=grandparent,
    )
    issue.parent = parent
    issue.save(update_fields=["parent"])

    ctx = build_context(issue, run)
    lineage = ctx["lineage"]
    assert lineage is not None
    # Ordered current -> parent -> grandparent (root).
    assert [n["title"] for n in lineage] == ["Make button blue", "Mid epic", "Root epic"]
    assert lineage[0]["identifier"] == ctx["issue"]["identifier"]
    assert lineage[-1]["title"] == "Root epic"


@pytest.mark.unit
def test_context_includes_project_description_when_set(
    workspace, create_user
):
    project = Project.objects.create(
        name="Documented Project",
        identifier="DP",
        workspace=workspace,
        created_by=create_user,
        description="Core backend services. Prefer additive migrations.",
    )
    project_state = State.objects.create(
        name="Todo", project=project, group="unstarted"
    )
    issue = Issue.objects.create(
        name="Fix a thing",
        workspace=workspace,
        project=project,
        state=project_state,
        created_by=create_user,
    )
    run = AgentRun.objects.create(
        owner=create_user, workspace=workspace, prompt="", work_item=issue
    )
    ctx = build_context(issue, run)
    assert (
        ctx["project"]["description"]
        == "Core backend services. Prefer additive migrations."
    )


@pytest.mark.unit
def test_context_project_description_defaults_to_empty_string(issue, run):
    # The `project` fixture above doesn't set `description`, so the model's
    # TextField(blank=True) default applies — must surface as "" (never None)
    # so the template's `{% if project.description %}` guard behaves.
    ctx = build_context(issue, run)
    assert ctx["project"]["description"] == ""


@pytest.mark.unit
def test_context_empty_base_branch_surfaces_as_none(workspace, create_user):
    # A project with no base_branch set — empty strings must flow through as
    # ``None`` so the prompt template takes the "auto-detect remote default"
    # branch instead of rendering a literal empty string.
    project = Project.objects.create(
        name="No Default",
        identifier="ND",
        workspace=workspace,
        created_by=create_user,
        repo_url="git@github.com:acme/no-default.git",
        base_branch="",
    )
    project_state = State.objects.create(name="Todo", project=project, group="unstarted")
    issue = Issue.objects.create(
        name="Fix a thing",
        workspace=workspace,
        project=project,
        state=project_state,
        created_by=create_user,
    )
    run = AgentRun.objects.create(
        owner=create_user, workspace=workspace, prompt="", work_item=issue
    )
    ctx = build_context(issue, run)
    assert ctx["repo"]["base_branch"] is None
    assert ctx["repo"]["work_branch"] is None


@pytest.mark.unit
def test_context_run_trigger_surfaced_from_field(issue, run):
    run.trigger = "tick"
    run.save(update_fields=["trigger"])
    ctx = build_context(issue, run)
    assert ctx["run"]["trigger"] == "tick"


@pytest.mark.unit
def test_context_tick_none_without_ticker(issue, run):
    ctx = build_context(issue, run)
    assert ctx["tick"] is None


@pytest.mark.unit
def test_context_tick_populated_from_ticker(issue, run):
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    IssueAgentTicker.objects.create(
        issue=issue, tick_count=5, interval_seconds=7200, max_ticks=24
    )
    ctx = build_context(issue, run)
    assert ctx["tick"] == {
        "count": 5,
        "cap": 24,
        "remaining": 19,
        "interval_seconds": 7200,
        "interval_human": "2 hours",
    }


@pytest.mark.unit
def test_context_tick_infinite_cap_surfaces_none(issue, run):
    # -1 means infinite — cap/remaining must surface as None so templates
    # can branch with `{% if tick.cap is not none %}`.
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    IssueAgentTicker.objects.create(issue=issue, tick_count=3, max_ticks=-1)
    ctx = build_context(issue, run)
    assert ctx["tick"]["cap"] is None
    assert ctx["tick"]["remaining"] is None
    assert ctx["tick"]["count"] == 3


@pytest.mark.unit
def test_context_tick_none_when_ticker_disarmed(issue, run):
    # A disarmed ticker (cap hit, user disabled, left the ticking state)
    # must not render the "automatically re-invokes" schedule block — the
    # promise would be false and invites the agent to defer work to a
    # tick that never fires.
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    IssueAgentTicker.objects.create(issue=issue, tick_count=5, enabled=False)
    ctx = build_context(issue, run)
    assert ctx["tick"] is None


@pytest.mark.unit
def test_context_tick_none_for_nonsense_interval(issue, run, project):
    # The project-default interval is API-writable with no validation;
    # "about every 0 hours" must not reach a prompt.
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    project.agent_default_interval_seconds = 0
    project.save(update_fields=["agent_default_interval_seconds"])
    IssueAgentTicker.objects.create(issue=issue, tick_count=1)
    ctx = build_context(issue, run)
    assert ctx["tick"] is None


@pytest.mark.unit
def test_context_tick_none_for_negative_noninfinite_cap(issue, run):
    # Only -1 means infinite; any other negative cap is misconfiguration
    # ("used 1 of -2 ticks") and the schedule block must be omitted.
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    IssueAgentTicker.objects.create(issue=issue, tick_count=1, max_ticks=-2)
    ctx = build_context(issue, run)
    assert ctx["tick"] is None


@pytest.mark.unit
def test_context_survives_run_without_trigger_attribute(issue):
    # The template-preview endpoint renders with a stub run that has no
    # ``trigger`` attribute — build_context must not raise, and trigger
    # surfaces as None so the "Why this run started" block is skipped.
    class _StubRun:
        def __init__(self):
            self.id = "00000000-0000-0000-0000-000000000000"
            self.work_item_id = None

    ctx = build_context(issue, _StubRun())
    assert ctx["run"]["trigger"] is None
