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
def test_context_parent_includes_state(workspace, project, state, create_user, run, issue):
    parent = Issue.objects.create(
        name="Umbrella epic",
        workspace=workspace,
        project=project,
        state=state,  # "Todo"
        created_by=create_user,
    )
    issue.parent = parent
    issue.save(update_fields=["parent"])

    ctx = build_context(issue, run)
    assert ctx["parent"]["state"] == "Todo"


# ----------------------------------------------------------------------
# Children (down) + relates_to siblings (across) — PDASHOSS01-160
# ----------------------------------------------------------------------


@pytest.mark.unit
def test_context_children_empty_when_none(issue, run):
    ctx = build_context(issue, run)
    assert ctx["children"] == []


@pytest.mark.unit
def test_context_includes_direct_children(workspace, project, state, create_user, run, issue):
    child_a = Issue.objects.create(
        name="Broken-out child A", workspace=workspace, project=project, state=state, created_by=create_user
    )
    child_b = Issue.objects.create(
        name="Broken-out child B", workspace=workspace, project=project, state=state, created_by=create_user
    )
    child_a.parent = issue
    child_a.save(update_fields=["parent"])
    child_b.parent = issue
    child_b.save(update_fields=["parent"])

    ctx = build_context(issue, run)
    children = ctx["children"]
    assert len(children) == 2
    titles = {c["title"] for c in children}
    assert titles == {"Broken-out child A", "Broken-out child B"}
    for c in children:
        assert c["identifier"].startswith("TP-")
        assert c["state"] == "Todo"


@pytest.mark.unit
def test_context_children_are_direct_only_not_grandchildren(workspace, project, state, create_user, run, issue):
    child = Issue.objects.create(
        name="Direct child", workspace=workspace, project=project, state=state, created_by=create_user, parent=issue
    )
    Issue.objects.create(
        name="Grandchild", workspace=workspace, project=project, state=state, created_by=create_user, parent=child
    )

    ctx = build_context(issue, run)
    titles = {c["title"] for c in ctx["children"]}
    assert titles == {"Direct child"}


@pytest.mark.unit
def test_context_related_empty_when_none(issue, run):
    ctx = build_context(issue, run)
    assert ctx["related"] == []


def _make_relation(issue, related, relation_type, create_user):
    from pi_dash.db.models import IssueRelation

    return IssueRelation.objects.create(
        issue=issue,
        related_issue=related,
        relation_type=relation_type,
        project=issue.project,
        workspace=issue.workspace,
        created_by=create_user,
    )


@pytest.mark.unit
def test_context_related_merges_both_directions(workspace, project, state, create_user, run, issue):
    # A relates_to link created from either side must surface, since the
    # relation is symmetric.
    other_a = Issue.objects.create(
        name="Linked from our side", workspace=workspace, project=project, state=state, created_by=create_user
    )
    other_b = Issue.objects.create(
        name="Linked from their side", workspace=workspace, project=project, state=state, created_by=create_user
    )
    _make_relation(issue, other_a, "relates_to", create_user)  # issue -> other_a
    _make_relation(other_b, issue, "relates_to", create_user)  # other_b -> issue

    ctx = build_context(issue, run)
    titles = {r["title"] for r in ctx["related"]}
    assert titles == {"Linked from our side", "Linked from their side"}
    for r in ctx["related"]:
        assert r["identifier"].startswith("TP-")
        assert r["state"] == "Todo"


@pytest.mark.unit
def test_context_related_excludes_other_relation_types(workspace, project, state, create_user, run, issue):
    # The related group is relates_to only; blocked_by has its own group
    # (PDASHOSS01-196) and duplicate is not surfaced at all.
    blocker = Issue.objects.create(
        name="Blocks us", workspace=workspace, project=project, state=state, created_by=create_user
    )
    dup = Issue.objects.create(
        name="Duplicate", workspace=workspace, project=project, state=state, created_by=create_user
    )
    _make_relation(issue, blocker, "blocked_by", create_user)
    _make_relation(issue, dup, "duplicate", create_user)

    ctx = build_context(issue, run)
    assert ctx["related"] == []


@pytest.mark.unit
def test_context_related_excludes_soft_deleted(workspace, project, state, create_user, run, issue):
    other = Issue.objects.create(
        name="Was related", workspace=workspace, project=project, state=state, created_by=create_user
    )
    rel = _make_relation(issue, other, "relates_to", create_user)
    rel.delete()  # soft delete

    ctx = build_context(issue, run)
    assert ctx["related"] == []


# ----------------------------------------------------------------------
# Directional relations (blocked_by / blocking / ...) — PDASHOSS01-196
# ----------------------------------------------------------------------


def _other_issue(workspace, project, state, create_user, name):
    return Issue.objects.create(name=name, workspace=workspace, project=project, state=state, created_by=create_user)


@pytest.mark.unit
def test_context_directional_relations_empty_when_none(issue, run):
    ctx = build_context(issue, run)
    assert ctx["blocked_by"] == []
    assert ctx["blocking"] == []
    assert ctx["other_relations"] == []
    assert ctx["open_blockers"] == []
    assert ctx["has_open_blockers"] is False


@pytest.mark.unit
def test_context_blocked_by_and_blocking_resolve_both_directions(workspace, project, state, create_user, run, issue):
    # Only the forward type is stored: (issue=A, related=B, "blocked_by") means
    # A is blocked by B, so B sees A as "blocking".
    blocker = _other_issue(workspace, project, state, create_user, "Blocks us")
    dependent = _other_issue(workspace, project, state, create_user, "Waits on us")
    _make_relation(issue, blocker, "blocked_by", create_user)
    _make_relation(dependent, issue, "blocked_by", create_user)

    ctx = build_context(issue, run)
    assert [b["title"] for b in ctx["blocked_by"]] == ["Blocks us"]
    assert [b["title"] for b in ctx["blocking"]] == ["Waits on us"]
    item = ctx["blocked_by"][0]
    assert item["identifier"] == f"TP-{blocker.sequence_id}"
    assert item["state"] == "Todo"
    assert item["state_group"] == "unstarted"
    # relates_to stays separate from the directional groups.
    assert ctx["related"] == []


@pytest.mark.unit
def test_context_other_relations_use_reverse_mapping(workspace, project, state, create_user, run, issue):
    ours = {
        "start_before": _other_issue(workspace, project, state, create_user, "We start before"),
        "finish_before": _other_issue(workspace, project, state, create_user, "We finish before"),
        "implemented_by": _other_issue(workspace, project, state, create_user, "Implements us"),
    }
    theirs = {
        "start_before": _other_issue(workspace, project, state, create_user, "Starts before us"),
        "finish_before": _other_issue(workspace, project, state, create_user, "Finishes before us"),
        "implemented_by": _other_issue(workspace, project, state, create_user, "Implemented by us"),
    }
    for relation_type, other in ours.items():
        _make_relation(issue, other, relation_type, create_user)
    for relation_type, other in theirs.items():
        _make_relation(other, issue, relation_type, create_user)

    ctx = build_context(issue, run)
    got = {(r["relation"], r["title"]) for r in ctx["other_relations"]}
    assert got == {
        ("Starts before", "We start before"),
        ("Starts after", "Starts before us"),
        ("Finishes before", "We finish before"),
        ("Finishes after", "Finishes before us"),
        ("Implemented by", "Implements us"),
        ("Implements", "Implemented by us"),
    }
    assert ctx["blocked_by"] == [] and ctx["blocking"] == []


@pytest.mark.unit
def test_context_directional_relations_exclude_relates_to_and_duplicate(
    workspace, project, state, create_user, run, issue
):
    _make_relation(issue, _other_issue(workspace, project, state, create_user, "Rel"), "relates_to", create_user)
    _make_relation(issue, _other_issue(workspace, project, state, create_user, "Dup"), "duplicate", create_user)

    ctx = build_context(issue, run)
    assert ctx["blocked_by"] == [] and ctx["blocking"] == [] and ctx["other_relations"] == []


@pytest.mark.unit
def test_context_blocked_by_lists_pair_once_and_skips_self(workspace, project, state, create_user, run, issue):
    blocker = _other_issue(workspace, project, state, create_user, "Blocker")
    rel = _make_relation(issue, blocker, "blocked_by", create_user)
    # A second live row for the same pair can't exist (unique constraint), so
    # soft-delete the first and re-link: the default manager hides the old row
    # and the live one must surface exactly once.
    rel.delete()
    _make_relation(issue, blocker, "blocked_by", create_user)
    # Self-link rows are skipped outright.
    _make_relation(issue, issue, "blocked_by", create_user)

    ctx = build_context(issue, run)
    assert [b["title"] for b in ctx["blocked_by"]] == ["Blocker"]
    assert ctx["blocking"] == []


@pytest.mark.unit
def test_context_blocked_by_capped(workspace, project, state, create_user, run, issue):
    from pi_dash.prompting.context import _MAX_RELATIONSHIP_ITEMS

    for i in range(_MAX_RELATIONSHIP_ITEMS + 3):
        _make_relation(issue, _other_issue(workspace, project, state, create_user, f"B{i}"), "blocked_by", create_user)

    ctx = build_context(issue, run)
    assert len(ctx["blocked_by"]) == _MAX_RELATIONSHIP_ITEMS
    assert len(ctx["open_blockers"]) == _MAX_RELATIONSHIP_ITEMS


@pytest.mark.unit
def test_context_blocked_by_skips_soft_deleted_relation_and_target(workspace, project, state, create_user, run, issue):
    gone_rel = _other_issue(workspace, project, state, create_user, "Relation deleted")
    gone_issue = _other_issue(workspace, project, state, create_user, "Issue deleted")
    _make_relation(issue, gone_rel, "blocked_by", create_user).delete()
    _make_relation(issue, gone_issue, "blocked_by", create_user)
    gone_issue.delete()  # soft delete the target work item

    ctx = build_context(issue, run)
    assert ctx["blocked_by"] == []
    assert ctx["has_open_blockers"] is False


@pytest.mark.unit
def test_context_has_open_blockers_tracks_blocker_state_group(workspace, project, state, create_user, run, issue):
    done = State.objects.create(name="Done", project=project, group="completed")
    cancelled = State.objects.create(name="Cancelled", project=project, group="cancelled")
    review = State.objects.create(name="In Review", project=project, group="review")
    closed_a = Issue.objects.create(
        name="Done one", workspace=workspace, project=project, state=done, created_by=create_user
    )
    closed_b = Issue.objects.create(
        name="Dropped one", workspace=workspace, project=project, state=cancelled, created_by=create_user
    )
    _make_relation(issue, closed_a, "blocked_by", create_user)
    _make_relation(issue, closed_b, "blocked_by", create_user)

    ctx = build_context(issue, run)
    assert len(ctx["blocked_by"]) == 2
    assert ctx["open_blockers"] == []
    assert ctx["has_open_blockers"] is False

    still_open = Issue.objects.create(
        name="Open one", workspace=workspace, project=project, state=review, created_by=create_user
    )
    _make_relation(issue, still_open, "blocked_by", create_user)

    ctx = build_context(issue, run)
    assert ctx["open_blockers"] == [f"TP-{still_open.sequence_id}"]
    assert ctx["has_open_blockers"] is True


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
def test_context_tick_populated_from_ticker(issue, run, project):
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    project.agent_default_interval_seconds = 7200
    project.agent_default_max_ticks = 10
    project.save(update_fields=["agent_default_interval_seconds", "agent_default_max_ticks"])
    IssueAgentTicker.objects.create(issue=issue, used=5)
    ctx = build_context(issue, run)
    assert ctx["tick"] == {
        "count": 5,
        "cap": 10,
        "remaining": 5,
        # Nothing has waited: ``count`` / ``cap`` are the pool as configured.
        "waited": 0,
        "wait_allowance": 10,
        "spent": False,
        "clock_live": True,
        "interval_seconds": 7200,
        "interval_human": "2 hours",
    }


@pytest.mark.unit
def test_context_tick_counts_retick_grants(issue, run, project):
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    project.agent_default_max_ticks = 10
    project.save(update_fields=["agent_default_max_ticks"])
    IssueAgentTicker.objects.create(issue=issue, used=10, granted=3)
    ctx = build_context(issue, run)
    assert ctx["tick"]["cap"] == 13
    assert ctx["tick"]["remaining"] == 3


@pytest.mark.unit
def test_context_tick_infinite_cap_surfaces_none(issue, run, project):
    # -1 means infinite — cap/remaining must surface as None so templates
    # can branch with `{% if tick.cap is not none %}`.
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    project.agent_default_max_ticks = -1
    project.save(update_fields=["agent_default_max_ticks"])
    IssueAgentTicker.objects.create(issue=issue, used=3)
    ctx = build_context(issue, run)
    assert ctx["tick"]["cap"] is None
    assert ctx["tick"]["remaining"] is None
    assert ctx["tick"]["count"] == 3
    assert ctx["tick"]["spent"] is False


@pytest.mark.unit
def test_context_tick_renders_when_ticker_stopped(issue, run):
    # A stopped clock (cap hit, terminal signal, user disabled) still
    # renders the budget line — the spent-pool case is exactly what the
    # agent must be warned about — but reports the clock as not live.
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    IssueAgentTicker.objects.create(issue=issue, used=5, enabled=False)
    ctx = build_context(issue, run)
    assert ctx["tick"]["count"] == 5
    assert ctx["tick"]["clock_live"] is False


@pytest.mark.unit
def test_context_tick_spent_means_no_run_follows_this_one(issue, run, project):
    """``used`` already counts the run being rendered when the ticker
    started it, so ``remaining == 0`` is exactly "no machine-started run
    follows" — there is no separate "last run" case to flag."""
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    project.agent_default_max_ticks = 10
    project.save(update_fields=["agent_default_max_ticks"])
    ticker = IssueAgentTicker.objects.create(issue=issue, used=9)
    ctx = build_context(issue, run)
    assert ctx["tick"]["spent"] is False
    assert ctx["tick"]["remaining"] == 1
    assert "last_run" not in ctx["tick"]
    ticker.used = 10
    ticker.save(update_fields=["used"])
    issue.refresh_from_db()
    ctx = build_context(issue, run)
    assert ctx["tick"]["spent"] is True
    assert ctx["tick"]["remaining"] == 0


@pytest.mark.unit
def test_context_tick_none_for_nonsense_interval(issue, run, project):
    # The project-default interval is API-writable with no validation;
    # "about every 0 hours" must not reach a prompt.
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    project.agent_default_interval_seconds = 0
    project.save(update_fields=["agent_default_interval_seconds"])
    IssueAgentTicker.objects.create(issue=issue, used=1)
    ctx = build_context(issue, run)
    assert ctx["tick"] is None


@pytest.mark.unit
def test_context_tick_none_for_negative_noninfinite_cap(issue, run, project):
    # Only -1 means infinite; any other negative cap is misconfiguration
    # ("used 1 of -2 runs") and the budget line must be omitted.
    from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker

    project.agent_default_max_ticks = -2
    project.save(update_fields=["agent_default_max_ticks"])
    IssueAgentTicker.objects.create(issue=issue, used=1)
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
