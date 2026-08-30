# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.db.models import (
    GitProviderAccount,
    GitRepository,
    GitRepositoryBinding,
    Issue,
    IssueComment,
    Project,
    State,
)
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
    return AgentRun.objects.create(owner=create_user, workspace=workspace, prompt="", work_item=issue)


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
    assert ctx["repo"]["code_review_term"] == "code review"
    # No git_work_branch set on the issue → should surface as None so templates
    # can branch on `{% if repo.work_branch %}` without false positives.
    assert ctx["repo"]["work_branch"] is None
    assert ctx["run"]["attempt"] == 1
    assert ctx["run"]["turn_number"] == 1


@pytest.mark.unit
def test_context_excludes_folded_comments(issue, run, workspace, project, create_user):
    IssueComment.objects.create(
        issue=issue,
        workspace=workspace,
        project=project,
        actor=create_user,
        comment_html="<p>Substantive update</p>",
    )
    IssueComment.objects.create(
        issue=issue,
        workspace=workspace,
        project=project,
        actor=create_user,
        comment_html="<p>No change from the last tick</p>",
        labels=["fold"],
    )

    comments_section = build_context(issue, run)["comments_section"]
    assert "Substantive update" in comments_section
    assert "No change from the last tick" not in comments_section


@pytest.mark.unit
def test_context_attempt_increments_on_follow_up(
    issue, run, workspace, create_user
):
    AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="prior",
        work_item=issue,
        status="completed",
    )
    ctx = build_context(issue, run)
    assert ctx["run"]["attempt"] == 2


@pytest.mark.unit
def test_context_code_reviews_empty_when_none_attached(issue, run):
    ctx = build_context(issue, run)
    assert ctx["code_reviews"] == []


@pytest.mark.unit
def test_context_includes_attached_code_reviews(issue, run):
    from pi_dash.db.models import GitCodeReviewLink

    GitCodeReviewLink.objects.create(
        issue=issue,
        project=issue.project,
        workspace=issue.workspace,
        provider="github",
        host_url="https://github.com",
        namespace="acme",
        repo_name="web",
        external_iid="42",
        url="https://github.com/acme/web/pull/42",
        title="Add feature",
        state="open",
        draft=True,
    )
    ctx = build_context(issue, run)
    assert len(ctx["code_reviews"]) == 1
    cr = ctx["code_reviews"][0]
    assert cr["url"] == "https://github.com/acme/web/pull/42"
    assert cr["title"] == "Add feature"
    assert cr["state"] == "open"
    assert cr["merged"] is False
    assert cr["draft"] is True
    assert cr["provider"] == "github"
    assert cr["external_iid"] == "42"


@pytest.mark.unit
def test_context_code_reviews_excludes_soft_deleted(issue, run):
    from pi_dash.db.models import GitCodeReviewLink

    link = GitCodeReviewLink.objects.create(
        issue=issue,
        project=issue.project,
        workspace=issue.workspace,
        provider="github",
        host_url="https://github.com",
        namespace="acme",
        repo_name="web",
        external_iid="43",
        url="https://github.com/acme/web/pull/43",
    )
    link.delete()  # soft delete
    ctx = build_context(issue, run)
    assert ctx["code_reviews"] == []


@pytest.mark.unit
def test_context_includes_git_work_branch_when_set(issue, run):
    issue.git_work_branch = "feat/pinned-branch"
    issue.save(update_fields=["git_work_branch"])
    ctx = build_context(issue, run)
    assert ctx["repo"]["work_branch"] == "feat/pinned-branch"


@pytest.mark.unit
def test_context_includes_bound_git_provider_details(workspace, project, issue, run, create_user):
    account = GitProviderAccount.objects.create(
        workspace=workspace,
        provider="gitlab",
        host_url="https://gitlab.com",
        auth_type="pat",
        external_account_id="u1",
        display_name="alice",
        credential_config={
            "token": "token",
            "host_url": "https://gitlab.com",
            "auth_type": "pat",
        },
    )
    repo = GitRepository.objects.create(
        provider="gitlab",
        host_url="https://gitlab.com",
        external_id="99",
        namespace="acme",
        name="web",
        full_name="acme/web",
        web_url="https://gitlab.com/acme/web",
    )
    GitRepositoryBinding.objects.create(
        project=project,
        workspace=workspace,
        repository=repo,
        provider_account=account,
        actor=create_user,
    )

    ctx = build_context(issue, run)

    assert ctx["repo"]["provider"] == "gitlab"
    assert ctx["repo"]["provider_display_name"] == "GitLab"
    assert ctx["repo"]["host_url"] == "https://gitlab.com"
    assert ctx["repo"]["full_name"] == "acme/web"
    assert ctx["repo"]["code_review_term"] == "merge request"


@pytest.mark.unit
def test_context_parent_is_none_when_unset(issue, run):
    ctx = build_context(issue, run)
    assert ctx["parent"] is None


@pytest.mark.unit
def test_context_parent_uses_parents_own_project_identifier(workspace, project, state, create_user, run, issue):
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
    other_state = State.objects.create(name="Todo", project=other_project, group="unstarted")
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
        f"parent identifier should use parent's project (OP), got {ctx['parent']['identifier']!r}"
    )
    assert ctx["parent"]["title"] == "Umbrella epic"
    assert ctx["parent"]["work_branch"] == "pi-dash/op-1"


@pytest.mark.unit
def test_context_parent_work_branch_empty_surfaces_as_none(workspace, project, state, create_user, run, issue):
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
def test_context_parent_includes_description_and_comment_count(workspace, project, state, create_user, run, issue):
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
def test_context_parent_code_reviews_empty_when_none(workspace, project, state, create_user, run, issue):
    # A parent with no attached PRs still exposes the key as an empty list so
    # the template can branch with `{% if parent.code_reviews %}`.
    parent = Issue.objects.create(
        name="Umbrella epic",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
    )
    issue.parent = parent
    issue.save(update_fields=["parent"])

    ctx = build_context(issue, run)
    assert ctx["parent"]["code_reviews"] == []


@pytest.mark.unit
def test_context_parent_includes_attached_code_reviews(workspace, project, state, create_user, run, issue):
    from pi_dash.db.models import GitCodeReviewLink

    parent = Issue.objects.create(
        name="Umbrella epic",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
        git_work_branch="pi-dash/parent-impl",
    )
    GitCodeReviewLink.objects.create(
        issue=parent,
        project=parent.project,
        workspace=parent.workspace,
        provider="github",
        host_url="https://github.com",
        namespace="acme",
        repo_name="web",
        external_iid="7",
        url="https://github.com/acme/web/pull/7",
        title="Parent groundwork",
        state="open",
    )
    issue.parent = parent
    issue.save(update_fields=["parent"])

    ctx = build_context(issue, run)
    reviews = ctx["parent"]["code_reviews"]
    assert len(reviews) == 1
    # Same shape as the issue's own code_reviews list.
    assert reviews[0]["url"] == "https://github.com/acme/web/pull/7"
    assert reviews[0]["title"] == "Parent groundwork"
    assert reviews[0]["state"] == "open"
    assert reviews[0]["merged"] is False
    assert reviews[0]["provider"] == "github"
    assert reviews[0]["external_iid"] == "7"


@pytest.mark.unit
def test_context_parent_code_reviews_excludes_soft_deleted(workspace, project, state, create_user, run, issue):
    from pi_dash.db.models import GitCodeReviewLink

    parent = Issue.objects.create(
        name="Umbrella epic",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
    )
    link = GitCodeReviewLink.objects.create(
        issue=parent,
        project=parent.project,
        workspace=parent.workspace,
        provider="github",
        host_url="https://github.com",
        namespace="acme",
        repo_name="web",
        external_iid="8",
        url="https://github.com/acme/web/pull/8",
    )
    link.delete()  # soft delete
    issue.parent = parent
    issue.save(update_fields=["parent"])

    ctx = build_context(issue, run)
    assert ctx["parent"]["code_reviews"] == []


def _make_parent_with_review(workspace, project, state, create_user, issue, *, work_branch, review=None):
    """Attach a parent (optionally with one PR) to ``issue`` and return it."""
    from pi_dash.db.models import GitCodeReviewLink

    parent = Issue.objects.create(
        name="Umbrella epic",
        workspace=workspace,
        project=project,
        state=state,
        created_by=create_user,
        git_work_branch=work_branch,
    )
    if review is not None:
        GitCodeReviewLink.objects.create(
            issue=parent,
            project=parent.project,
            workspace=parent.workspace,
            provider="github",
            host_url="https://github.com",
            namespace="acme",
            repo_name="web",
            external_iid=review.get("external_iid", "9"),
            url=review.get("url", "https://github.com/acme/web/pull/9"),
            title=review.get("title", "Parent PR"),
            state=review.get("state", "open"),
            merged=review.get("merged", False),
        )
    issue.parent = parent
    issue.save(update_fields=["parent"])
    return parent


def _render_section(key, ctx):
    from pi_dash.prompting import registry
    from pi_dash.prompting.renderer import render

    return render(registry.get_section(key).default_body, ctx)


@pytest.mark.unit
def test_intro_renders_parent_pr_block_only_when_non_empty(workspace, project, state, create_user, run, issue):
    # Parent without PRs → the parent-PR block is omitted.
    _make_parent_with_review(workspace, project, state, create_user, issue, work_branch="pi-dash/parent-impl")
    out = _render_section("intro", build_context(issue, run))
    assert "may have already implemented part of this work" not in out

    # Parent with a PR → the block (and the PR line) render.
    from pi_dash.db.models import GitCodeReviewLink

    GitCodeReviewLink.objects.create(
        issue=issue.parent,
        project=issue.parent.project,
        workspace=issue.parent.workspace,
        provider="github",
        host_url="https://github.com",
        namespace="acme",
        repo_name="web",
        external_iid="11",
        url="https://github.com/acme/web/pull/11",
        title="Parent groundwork",
        state="open",
    )
    out = _render_section("intro", build_context(issue, run))
    assert "may have already implemented part of this work" in out
    assert "https://github.com/acme/web/pull/11" in out
    assert "Parent groundwork" in out


@pytest.mark.unit
def test_workpad_setup_base_stacks_on_parent_with_open_pr(workspace, project, state, create_user, run, issue):
    _make_parent_with_review(
        workspace, project, state, create_user, issue,
        work_branch="pi-dash/parent-impl",
        review={"state": "open", "merged": False},
    )
    out = _render_section("workpad-setup", build_context(issue, run))
    assert "BASE=pi-dash/parent-impl" in out
    assert "BASE=trunk" not in out


@pytest.mark.unit
def test_workpad_setup_base_falls_back_when_parent_pr_merged(workspace, project, state, create_user, run, issue):
    _make_parent_with_review(
        workspace, project, state, create_user, issue,
        work_branch="pi-dash/parent-impl",
        review={"state": "closed", "merged": True},
    )
    out = _render_section("workpad-setup", build_context(issue, run))
    # Parent PR merged → do not stack on the (now-merged) parent branch.
    assert "BASE=trunk" in out
    assert "BASE=pi-dash/parent-impl" not in out


@pytest.mark.unit
def test_workpad_setup_base_stacks_on_parent_branch_without_pr(workspace, project, state, create_user, run, issue):
    _make_parent_with_review(workspace, project, state, create_user, issue, work_branch="pi-dash/parent-impl")
    out = _render_section("workpad-setup", build_context(issue, run))
    # Branch present, no attached PR → branch is the only signal, so stack.
    assert "BASE=pi-dash/parent-impl" in out
    assert "BASE=trunk" not in out


@pytest.mark.unit
def test_implementation_base_reflects_parent_pr_state(workspace, project, state, create_user, run, issue):
    # Open parent PR → child PR base is the parent branch.
    _make_parent_with_review(
        workspace, project, state, create_user, issue,
        work_branch="pi-dash/parent-impl",
        review={"state": "open", "merged": False},
    )
    out = _render_section("implementation", build_context(issue, run))
    assert "pi-dash/parent-impl" in out

    # Merged parent PR → child PR base falls back to the project base branch.
    issue.parent.git_code_reviews.update(state="closed", merged=True)
    out = _render_section("implementation", build_context(issue, run))
    assert "`trunk`" in out
    assert "pi-dash/parent-impl" not in out


@pytest.mark.unit
def test_context_lineage_is_none_for_single_parent(workspace, project, state, create_user, run, issue):
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
def test_context_lineage_populated_for_grandparent(workspace, project, state, create_user, run, issue):
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
def test_context_includes_project_description_when_set(workspace, create_user):
    project = Project.objects.create(
        name="Documented Project",
        identifier="DP",
        workspace=workspace,
        created_by=create_user,
        description="Core backend services. Prefer additive migrations.",
    )
    project_state = State.objects.create(name="Todo", project=project, group="unstarted")
    issue = Issue.objects.create(
        name="Fix a thing",
        workspace=workspace,
        project=project,
        state=project_state,
        created_by=create_user,
    )
    run = AgentRun.objects.create(owner=create_user, workspace=workspace, prompt="", work_item=issue)
    ctx = build_context(issue, run)
    assert ctx["project"]["description"] == "Core backend services. Prefer additive migrations."


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
    run = AgentRun.objects.create(owner=create_user, workspace=workspace, prompt="", work_item=issue)
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

    IssueAgentTicker.objects.create(issue=issue, tick_count=5, interval_seconds=7200, max_ticks=24)
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
