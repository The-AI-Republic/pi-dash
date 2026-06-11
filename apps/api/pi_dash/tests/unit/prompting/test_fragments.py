# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the fragment assembler.

Covers the assembly contract directly (ordering, required section headers,
glob scope) so regressions in fragment layout surface without needing the
full composer + Issue fixture stack.
"""

from __future__ import annotations

from uuid import uuid4

import pytest

from pi_dash.prompting.fragments import FRAGMENTS_DIR, assemble, fragment_paths
from pi_dash.prompting.renderer import render


@pytest.mark.unit
def test_fragment_paths_sorted_and_nonempty():
    paths = fragment_paths()
    assert paths, "no fragments discovered"
    names = [p.name for p in paths]
    assert names == sorted(names), f"fragments not in lexical order: {names}"


@pytest.mark.unit
def test_fragment_paths_glob_rejects_non_numeric_prefix(tmp_path, monkeypatch):
    # A stray README.md in the fragments directory must not get concatenated
    # into the production prompt. Simulate it by pointing FRAGMENTS_DIR at a
    # tempdir mirror with one valid and one invalid filename.
    (tmp_path / "01_ok.md").write_text("valid", encoding="utf-8")
    (tmp_path / "README.md").write_text("should be ignored", encoding="utf-8")
    (tmp_path / "NOTES.md").write_text("should be ignored", encoding="utf-8")

    from pi_dash.prompting import fragments as fragments_mod

    monkeypatch.setattr(fragments_mod, "FRAGMENTS_DIR", tmp_path)
    discovered = [p.name for p in fragments_mod.fragment_paths()]
    assert discovered == ["01_ok.md"]


@pytest.mark.unit
def test_assemble_orders_sections_and_includes_cli_section():
    body = assemble()

    # Section headers in expected order.
    expected_order = [
        "## Session framing",
        "## Pi Dash CLI",
        "## Default posture",
        "## Autonomy / escalation model",
        "## State routing",
        "## Step 1 — Workpad setup",
        "## Step 2 — Implementation and validation",
        "## Blocking the run",
        "## Guardrails",
        "## Workpad template",
        "## Ending the run",
    ]
    positions = [body.find(h) for h in expected_order]
    assert all(p >= 0 for p in positions), (
        f"missing section headers: "
        f"{[h for h, p in zip(expected_order, positions) if p < 0]}"
    )
    assert positions == sorted(positions), (
        f"section headers out of order: "
        f"{list(zip(expected_order, positions))}"
    )


@pytest.mark.unit
def test_assemble_contains_expanded_pidash_cli_subsections():
    body = assemble()
    # Subsections added when the CLI fragment was expanded — a regression
    # here means somebody dropped or reordered CLI guidance.
    for marker in (
        "### Environment",
        "### Output contract",
        "### Not for you",
        "### Typical recipes",
        "pidash comment update <identifier> <comment-id>",
        # Issue create/list are exposed to the agent — guard against silent
        # removal so the agent loses these capabilities.
        "pidash issue create --project",
        "pidash issue list --project",
    ):
        assert marker in body, f"missing CLI subsection marker: {marker!r}"


@pytest.mark.unit
def test_pr_work_routes_to_review_group_not_done():
    """A `code_change` that opens a PR must land in the `review` group
    ("In Review"), not `completed`/"Done".

    Regression for runs that marked PR-producing issues "Done": the prompt
    used to give ``--state "Done"`` as the *only* canonical success example,
    so a literal-minded agent (Codex) copied it and skipped In Review, while
    a judgement-driven agent (Claude) overrode it. The routing is now stated
    explicitly so neither has to infer it.
    """
    body = assemble()

    # The PR/success path must offer the review-group move...
    assert '--state "In Review"' in body, "no In Review routing in assembled prompt"
    # ...and it must appear before the Done example (In Review is the primary
    # ending for a run that opened a PR; Done is the noncode fallback).
    review_idx = body.find('--state "In Review"')
    done_idx = body.find('--state "Done"')
    assert review_idx != -1 and done_idx != -1
    assert review_idx < done_idx, (
        "Done is presented before In Review — In Review must be the primary "
        "ending for PR-producing work"
    )
    # The default posture and ending-run guidance both name the review group
    # as the destination for code-change/PR work.
    assert "`review` group" in body


@pytest.mark.unit
def test_assemble_preserves_unindented_jinja_block_tags():
    # Renderer runs with lstrip_blocks=False, so leading whitespace on
    # {% if %} / {% endif %} / {% else %} / {% endfor %} lines bleeds into
    # rendered output. Block tags must start at column 0.
    body = assemble()
    for line_no, line in enumerate(body.splitlines(), start=1):
        stripped = line.lstrip()
        if stripped.startswith("{%") and stripped != line:
            # Allow the nested `{% if repo.base_branch %}...{% endif %}` on
            # the "Resolve the base branch" bullet, which is an inline
            # expression, not a block tag on its own line.
            assert "{% if repo.base_branch %}" in line, (
                f"fragment line {line_no} has indented block tag: {line!r}"
            )


@pytest.mark.unit
def test_fragments_directory_exists_on_disk():
    assert FRAGMENTS_DIR.is_dir(), f"fragments dir missing: {FRAGMENTS_DIR}"


def _ctx_from_db(
    *,
    parent_branch=None,
    work_branch=None,
    base_branch="trunk",
    has_parent=False,
    project_description="",
    parent_description="",
    parent_comment_count=0,
    has_grandparent=False,
):
    """Build the Jinja context for these fragment tests via the production
    ``prompting.context.build_context``.

    Why go through the DB instead of a hand-rolled dict:

    The Jinja renderer runs in ``StrictUndefined`` mode (templates are
    workspace-admin-editable, so a typo in production must error rather
    than silently emit empty). A hand-rolled fixture has to enumerate
    every key any fragment ever references — adding a new
    ``{{ issue.foo }}`` to any fragment immediately breaks this test
    until the fixture is updated by hand. We've burned that paper cut
    a few times already.

    Sourcing the ctx from ``build_context(issue, run)`` instead means:

    - The fixture stays in lockstep with the production ctx shape
      automatically: whatever keys ``build_context`` adds, this test
      sees on the next run.
    - Templates that reference a key ``build_context`` doesn't fill in
      surface as a clear failure here (the right place for it), not
      hidden behind a stale fixture.

    The cost is touching the DB for this test, which is fine — the
    test class already runs under pytest-django.
    """
    from pi_dash.db.models import (
        Issue,
        IssueComment,
        Project,
        State,
        Workspace,
        User,
    )
    from pi_dash.runner.models import AgentRun, Pod
    from pi_dash.prompting.context import build_context

    # Each invocation rolls its own row set so parametrised tests don't
    # collide on unique (workspace_slug, project_identifier, …).
    owner, _ = User.objects.get_or_create(
        email="fragment-test@example.com",
        defaults={"username": "fragment-test"},
    )
    workspace = Workspace.objects.create(
        slug=f"ws-{uuid4().hex[:8]}",
        name="Fragments WS",
        owner=owner,
    )
    project = Project.objects.create(
        name="Test Project",
        identifier="TP",
        workspace=workspace,
        created_by=owner,
        base_branch=base_branch,
        description=project_description,
    )
    State.objects.create(
        name="Todo", project=project, workspace=workspace, group="unstarted", default=True
    )

    parent = None
    if has_parent:
        grandparent = None
        if has_grandparent:
            grandparent = Issue.objects.create(
                name="Root epic",
                workspace=workspace,
                project=project,
                created_by=owner,
            )
        parent = Issue.objects.create(
            name="Umbrella",
            workspace=workspace,
            project=project,
            created_by=owner,
            git_work_branch=parent_branch or "",
            parent=grandparent,
            description_html=(f"<p>{parent_description}</p>" if parent_description else ""),
        )
        for n in range(parent_comment_count):
            IssueComment.objects.create(
                issue=parent,
                workspace=workspace,
                project=project,
                created_by=owner,
                comment_html=f"<p>comment {n}</p>",
            )
    issue = Issue.objects.create(
        name="Make button blue",
        workspace=workspace,
        project=project,
        created_by=owner,
        parent=parent,
        git_work_branch=work_branch or "",
        priority="high",
    )
    pod = Pod.default_for_project(project)
    run = AgentRun.objects.create(
        work_item=issue,
        workspace=workspace,
        pod=pod,
        created_by=owner,
    )
    return build_context(issue, run)


@pytest.mark.unit
@pytest.mark.django_db
def test_assemble_renders_independent_issue_uses_project_base():
    body = render(assemble(), _ctx_from_db())
    # No parent → independent path: BASE comes from project base branch.
    assert "BASE=trunk" in body
    # And the PR-base prose in Step 2 matches.
    assert "`trunk`" in body


@pytest.mark.unit
@pytest.mark.django_db
def test_assemble_renders_parent_with_work_branch_uses_parent_branch():
    body = render(
        assemble(),
        _ctx_from_db(has_parent=True, parent_branch="pi-dash/tp-1"),
    )
    # Parent w/ branch → BASE points at the parent's branch.
    assert "BASE=pi-dash/tp-1" in body
    # PR-base prose in Step 2 references the parent's branch, not the project base.
    assert "`pi-dash/tp-1`" in body


@pytest.mark.unit
@pytest.mark.django_db
def test_assemble_renders_parent_without_work_branch_falls_back_to_project_base():
    body = render(assemble(), _ctx_from_db(has_parent=True, parent_branch=None))
    # Parent w/o branch → fall back to project base.
    assert "BASE=trunk" in body
    # And the fallback note must be present so the agent records it.
    assert "Fall back to the project base branch" in body


@pytest.mark.unit
@pytest.mark.django_db
def test_assemble_renders_existing_work_branch_skips_base_resolution():
    body = render(assemble(), _ctx_from_db(work_branch="feat/pinned"))
    # repo.work_branch path → checkout existing branch directly, no BASE= resolution.
    assert "git checkout feat/pinned" in body
    assert "BASE=" not in body


@pytest.mark.unit
@pytest.mark.django_db
def test_assemble_renders_no_parent_context_for_independent_issue():
    body = render(assemble(), _ctx_from_db())
    # Independent issue → no parent-context block at all.
    assert "Parent issue context" not in body


@pytest.mark.unit
@pytest.mark.django_db
def test_assemble_renders_parent_context_block_with_content_and_comment_count():
    body = render(
        assemble(),
        _ctx_from_db(
            has_parent=True,
            parent_description="Parent acceptance criteria here.",
            parent_comment_count=3,
        ),
    )
    assert "Parent issue context (this issue is a sub-issue of TP-" in body
    # Parent content is inlined...
    assert "Parent acceptance criteria here." in body
    # ...but the parent's comment bodies are not — only the count + the exact
    # ready-to-run command to read them.
    assert "has 3 comment(s)" in body
    assert "pidash comment list TP-" in body
    # Single parent (no grandparent) → no lineage tree.
    assert "multi-level parent lineage" not in body


@pytest.mark.unit
@pytest.mark.django_db
def test_assemble_renders_lineage_tree_when_grandparent_exists():
    body = render(
        assemble(),
        _ctx_from_db(has_parent=True, has_grandparent=True),
    )
    # Lineage tree is emitted with the current issue marked and arrows between
    # ancestors, ending at the root.
    assert "multi-level parent lineage" in body
    assert "(current)" in body
    assert "→" in body
    assert "Root epic" in body
    # And the agent is pointed at the CLI to fetch any ancestor's details.
    assert "pidash issue get <ANCESTOR-ID>" in body


@pytest.mark.unit
@pytest.mark.django_db
def test_assemble_renders_project_header_and_description_when_set():
    body = render(
        assemble(),
        _ctx_from_db(project_description="Core backend services."),
    )
    # The project header always renders.
    assert "Project: Test Project (TP)" in body
    # When description is set, it renders verbatim on its own line,
    # framed by blank lines between the Project header and Issue Description.
    assert "\nCore backend services.\n" in body
    assert (
        "Project: Test Project (TP)\n\nCore backend services.\n\nIssue Description:"
        in body
    )


@pytest.mark.unit
@pytest.mark.django_db
def test_assemble_renders_project_header_without_description_has_no_extra_blank_line():
    # Whitespace regression guard: with description empty, the conditional
    # block must not leave a double-blank-line gap between the Project header
    # and the Issue Description heading. renderer.py uses trim_blocks=False,
    # so the structure of fragments/01_intro.md around the {% if %} block is
    # what enforces single-blank-line spacing here.
    body = render(assemble(), _ctx_from_db(project_description=""))
    assert "Project: Test Project (TP)" in body
    assert "Project: Test Project (TP)\n\nIssue Description:" in body
    # Triple-newline would mean a double blank line — that's the regression.
    assert "Project: Test Project (TP)\n\n\nIssue Description:" not in body
