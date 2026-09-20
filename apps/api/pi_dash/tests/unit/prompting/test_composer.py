# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Composer: assembly, override resolution, manifest, and error attribution."""

from __future__ import annotations

import copy

import pytest

from pi_dash.prompting import recipes
from pi_dash.prompting.composer import (
    SOURCE_DEFAULT,
    SOURCE_DRAFT,
    SOURCE_WORKSPACE,
    compile_template,
    compose,
    resolve_section,
)
from pi_dash.prompting.models import PromptSectionOverride
from pi_dash.prompting.renderer import PromptRenderError
from pi_dash.prompting.validation import sample_contexts


def _ctx(kind="coding-task"):
    return sample_contexts(kind)[0]


# ----------------------------------------------------------------------
# Assembly + manifest
# ----------------------------------------------------------------------


@pytest.mark.unit
def test_compose_coding_task_renders_no_leftover_jinja():
    out = compose(
        "coding-task", workspace=None, project=None, user=None, context=_ctx()
    )
    assert "{%" not in out.text and "{{" not in out.text
    assert "orchestrates AI agents" in out.text  # stable intro phrase


@pytest.mark.unit
@pytest.mark.parametrize("executor", ["local_runner", "managed_runner"])
def test_repo_free_task_prompt_does_not_require_git(executor):
    ctx = _ctx()
    ctx["repo"]["url"] = ""
    out = compose(
        "coding-task", workspace=None, project=None, user=None,
        context=ctx, executor_kind=executor,
    ).text
    assert "A Git repository is optional" in out
    assert "ordinary folder, not a Git repository" in out
    assert "Do not initialize a repository" in out


@pytest.mark.unit
def test_manifest_one_entry_per_recipe_section_all_default():
    out = compose(
        "coding-task", workspace=None, project=None, user=None, context=_ctx()
    )
    recipe = recipes.recipe_for("coding-task")
    assert [e.section_key for e in out.manifest] == list(recipe)
    assert all(e.source == SOURCE_DEFAULT and e.version == 0 for e in out.manifest)


@pytest.mark.unit
def test_manifest_line_ranges_are_ordered_and_gapped():
    out = compose(
        "coding-task", workspace=None, project=None, user=None, context=_ctx()
    )
    prev_end = 0
    for e in out.manifest:
        assert e.line_start > prev_end  # strictly after previous (blank-line gap)
        assert e.line_end >= e.line_start
        prev_end = e.line_end


@pytest.mark.unit
def test_compose_applies_draft_override_for_preview():
    recipe = recipes.recipe_for("coding-task")
    target = recipe[0]
    out = compose(
        "coding-task",
        workspace=None,
        project=None,
        user=None,
        context=_ctx(),
        draft_overrides={target: "DRAFT-PREVIEW-MARKER"},
    )
    assert "DRAFT-PREVIEW-MARKER" in out.text
    entry = next(e for e in out.manifest if e.section_key == target)
    assert entry.source == SOURCE_DRAFT


@pytest.mark.unit
def test_compose_ignores_draft_override_outside_recipe():
    # A draft for a key not in this recipe must not alter the output.
    out = compose(
        "coding-task",
        workspace=None,
        project=None,
        user=None,
        context=_ctx(),
        draft_overrides={"not-a-real-section": "SHOULD-NOT-APPEAR"},
    )
    assert "SHOULD-NOT-APPEAR" not in out.text
    assert all(e.source == SOURCE_DEFAULT for e in out.manifest)


@pytest.mark.unit
def test_compile_template_keeps_jinja_markers():
    compiled = compile_template("coding-task", workspace=None, project=None, user=None)
    assert "{{ issue.identifier }}" in compiled.template_body
    assert compiled.text == compiled.template_body  # not rendered


# ----------------------------------------------------------------------
# Resolution precedence (design §6.2)
# ----------------------------------------------------------------------


@pytest.mark.unit
def test_resolve_section_default_when_no_override(db, workspace):
    r = resolve_section("implementation", workspace=workspace, project=None, user=None)
    assert r.source == SOURCE_DEFAULT
    assert r.version == 0


@pytest.mark.unit
def test_resolve_section_workspace_override_applies(db, workspace, create_user):
    PromptSectionOverride.objects.create(
        workspace=workspace,
        user=None,
        section_key="implementation",
        body="WS OVERRIDE BODY",
        updated_by=create_user,
    )
    r = resolve_section("implementation", workspace=workspace, project=None, user=None)
    assert r.source == SOURCE_WORKSPACE
    assert r.body == "WS OVERRIDE BODY"


@pytest.mark.unit
def test_resolve_section_user_override_beats_workspace(db, workspace, create_user):
    PromptSectionOverride.objects.create(
        workspace=workspace, user=None, section_key="implementation", body="WS"
    )
    PromptSectionOverride.objects.create(
        workspace=workspace, user=create_user, section_key="implementation", body="MINE"
    )
    r = resolve_section(
        "implementation", workspace=workspace, project=None, user=create_user
    )
    assert r.source == f"user:{create_user.id}"
    assert r.body == "MINE"


@pytest.mark.unit
def test_resolve_section_locked_ignores_overrides(db, workspace, create_user):
    # Even if a row somehow exists, a locked section never consults the chain.
    PromptSectionOverride.objects.create(
        workspace=workspace, user=None, section_key="pidash-cli", body="HACKED"
    )
    r = resolve_section(
        "pidash-cli", workspace=workspace, project=None, user=create_user
    )
    assert r.source == SOURCE_DEFAULT
    assert "HACKED" not in r.body


@pytest.mark.unit
def test_resolve_section_inactive_override_ignored(db, workspace):
    PromptSectionOverride.objects.create(
        workspace=workspace,
        user=None,
        section_key="implementation",
        body="OLD",
        is_active=False,
    )
    r = resolve_section("implementation", workspace=workspace, project=None, user=None)
    assert r.source == SOURCE_DEFAULT


@pytest.mark.unit
def test_compose_applies_workspace_override_in_output(db, workspace):
    PromptSectionOverride.objects.create(
        workspace=workspace,
        user=None,
        section_key="implementation",
        body="CUSTOM IMPLEMENTATION GUIDANCE",
    )
    out = compose(
        "coding-task",
        workspace=workspace,
        project=None,
        user=None,
        context=_ctx(),
    )
    assert "CUSTOM IMPLEMENTATION GUIDANCE" in out.text
    impl = next(e for e in out.manifest if e.section_key == "implementation")
    assert impl.source == SOURCE_WORKSPACE


# ----------------------------------------------------------------------
# Error attribution (design §6.3)
# ----------------------------------------------------------------------


@pytest.mark.unit
def test_render_error_attributes_failing_section(db, workspace):
    PromptSectionOverride.objects.create(
        workspace=workspace,
        user=None,
        section_key="implementation",
        body="{{ does_not_exist_variable }}",
    )
    with pytest.raises(PromptRenderError) as exc:
        compose(
            "coding-task",
            workspace=workspace,
            project=None,
            user=None,
            context=_ctx(),
        )
    # Either the precise section, or at least the active-override list.
    assert "implementation" in str(exc.value)


# ----------------------------------------------------------------------
# Golden output per kind (defaults only)
# ----------------------------------------------------------------------


@pytest.mark.unit
@pytest.mark.parametrize("kind", ["coding-task", "review", "test", "scheduler"])
def test_all_kinds_render_against_both_sample_contexts(kind):
    for ctx in sample_contexts(kind):
        out = compose(kind, workspace=None, project=None, user=None, context=ctx)
        assert out.text.strip()
        assert "{%" not in out.text and "{{" not in out.text


@pytest.mark.unit
def test_test_kind_renders_cycle_and_cli_docs():
    out = compose("test", workspace=None, project=None, user=None, context=_ctx("test"))
    # The polymorphic test cycle enumerates its five kinds and reports
    # results as a structured comment.
    assert "AUTOMATED" in out.text
    assert "NON_TECHNICAL" in out.text
    assert "structured results comment" in out.text
    # And carries the CLI docs like the other issue kinds.
    assert "Pi Dash CLI" in out.text


@pytest.mark.unit
def test_test_kind_frames_agent_as_first_user():
    """The test agent's identity is the change's first user: verdicts come
    from acting and observing on the consumer's side of the surface, with a
    regression smoke, and CI/gates demoted to corroborating evidence."""
    out = compose("test", workspace=None, project=None, user=None, context=_ctx("test"))
    body = out.text
    assert "first user" in body
    assert "regression smoke" in body
    # Pipelines corroborate, they never conclude the verdict.
    assert "corroborating evidence, never the verdict" in body
    # An environment is a means to act as the user, not the goal.
    assert "not the verdict" in body


@pytest.mark.unit
def test_test_kind_completed_stays_in_test_not_done():
    """A `completed` test pass leaves the issue In Test — the runner never
    moves a test issue to `completed`/"Done". The ending-run section must
    route the test kind to the Test-cycle Step 3, not the coding "move to
    In Review" text or a `--state "Done"`."""
    out = compose("test", workspace=None, project=None, user=None, context=_ctx("test"))
    body = out.text
    assert "leaves the issue In Test" in body or "leave the issue In Test" in body
    assert '--state "Done"' not in body


@pytest.mark.unit
def test_coding_task_posts_acceptance_criteria_handoff():
    """The impl run must hand the test phase a spec. In Test starts from a
    fresh session, so a comment with fixed headings — not the run summary —
    is the channel the next phase reliably inherits."""
    out = compose(
        "coding-task", workspace=None, project=None, user=None,
        context=_ctx("coding-task"),
    )
    body = out.text
    assert "### Acceptance Criteria" in body
    assert "### How to Test" in body
    # Reinforced in the exit checklist, not only in the implementation step.
    assert body.count("### Acceptance Criteria") >= 1
    assert "hand-off" in body


@pytest.mark.unit
def test_test_kind_reads_the_handoff_comment_first():
    out = compose(
        "test", workspace=None, project=None, user=None, context=_ctx("test")
    )
    body = out.text
    assert "hand-off comment" in body
    assert "### Acceptance Criteria" in body


@pytest.mark.unit
def test_test_kind_does_not_prescribe_a_package_manager():
    """Pi Dash runs against arbitrary customer repos — the AUTOMATED branch
    must discover the repo's gates, not assume a JS toolchain."""
    out = compose(
        "test", workspace=None, project=None, user=None, context=_ctx("test")
    )
    body = out.text
    for tool in ("pnpm", "yarn "):
        assert tool not in body
    assert "discover them" in body


@pytest.mark.unit
def test_review_default_includes_cli_docs():
    out = compose("review", workspace=None, project=None, user=None, context=_ctx("review"))
    assert "Pi Dash CLI" in out.text
    assert "pidash workpad" in out.text


# ----------------------------------------------------------------------
# State-routing regression: PR work ends in `review`, not `completed`
# ----------------------------------------------------------------------


@pytest.mark.unit
def test_coding_task_never_routes_to_done():
    """A finished coding-task run — PR or `noncode` — lands in the `review`
    group ("In Review"). The runner must NEVER proactively move an issue to
    `completed`/"Done" (PDASHOSS01-68): marking a question/debug/investigation
    "Done" drops it off the user's radar prematurely.

    Regression for runs that marked issues "Done": the prompt used to give
    ``--state "Done"`` as the canonical noncode-success example, so agents
    finishing a question/investigation copied it and closed the issue. The
    only success routing is now In Review, stated explicitly across
    pidash-cli / default-posture / implementation / ending-run.
    """
    out = compose(
        "coding-task", workspace=None, project=None, user=None, context=_ctx()
    )
    body = out.text

    # The success path routes to the review group...
    assert '--state "In Review"' in body, "no In Review routing in assembled prompt"
    # ...and Done is never offered as a runner success ending.
    assert '--state "Done"' not in body, (
        "coding-task prompt still offers `--state \"Done\"` as a success "
        "ending — the runner must never proactively move an issue to Done"
    )
    # The default posture and ending-run guidance both name the review group
    # as the destination for finished work.
    assert "`review` group" in body
    # And the sample project actually exposes a review-group state, so the
    # happy path (route to it) is real here, not just the no-review fallback.
    assert "(group: `review`)" in body


@pytest.mark.unit
def test_review_kind_approved_stays_in_review_not_done():
    """An approved review pass leaves the issue In Review — the runner never
    moves a review issue to `completed`/"Done" (PDASHOSS01-68, Comments 2 & 3).
    A human (or a separate supporting process) closes it. So the review-kind
    prompt must route to In Review and must not offer ``--state "Done"`` as a
    success ending.
    """
    out = compose("review", workspace=None, project=None, user=None, context=_ctx("review"))
    body = out.text

    # An approved review hands the task on to In Test; defects go back to
    # In Progress with open items...
    assert '--state "In Test"' in body
    assert '--state "In Progress"' in body
    # ...and never to Done.
    assert '--state "Done"' not in body
    # The lifecycle is shared and the run reports its outcome.
    assert "Task lifecycle" in body
    assert "pidash run yield --outcome" in body


# ----------------------------------------------------------------------
# Run-trigger / ticking guidance in the shared session-framing section
# ----------------------------------------------------------------------


@pytest.mark.unit
def test_session_framing_renders_tick_guidance_and_schedule():
    ctx = _ctx("coding-task")  # populated sample: trigger="tick", tick budget set
    out = compose(
        "coding-task", workspace=None, project=None, user=None, context=ctx
    ).text
    assert "automatically by the issue's ticker" in out
    assert "used 5 of 10 agent runs" in out
    assert "(5 remaining)" in out
    # The lifecycle section carries the budget line and the pool rules.
    assert "Runs used on this issue: **5 of 10** (5 remaining)" in out
    assert "about every 3 hours" in out


@pytest.mark.unit
def test_session_framing_review_tick_reports_done_not_noop():
    ctx = _ctx("review")
    out = compose(
        "review", workspace=None, project=None, user=None, context=ctx
    ).text
    assert "automatically by the issue's ticker" in out
    assert "emit `noop`" not in out
    assert "pidash run yield --outcome done" in out


@pytest.mark.unit
def test_lifecycle_warns_when_the_pool_is_spent():
    ctx = _ctx("review")
    ctx["tick"] = {
        **ctx["tick"],
        "count": 10,
        "cap": 10,
        "remaining": 0,
        "spent": True,
        "clock_live": False,
    }
    out = compose("review", workspace=None, project=None, user=None, context=ctx).text
    assert "The pool is spent" in out
    assert "No agent run will follow this one" in out
    assert "Re-tick" in out


@pytest.mark.unit
def test_lifecycle_spent_branch_covers_the_last_run(kind="coding-task"):
    ctx = _ctx(kind)
    ctx["tick"] = {**ctx["tick"], "count": 10, "cap": 10, "remaining": 0, "spent": True, "clock_live": False}
    out = compose(kind, workspace=None, project=None, user=None, context=ctx).text
    assert "this is the last run" in out
    assert "Never press Re-tick yourself" in out
    assert "from Paused" in out


@pytest.mark.unit
def test_cli_docs_put_re_tick_out_of_the_agents_hands():
    out = compose("coding-task", workspace=None, project=None, user=None, context=_ctx("coding-task")).text
    assert "`pidash issue re-tick`" in out
    assert "refuses a re-tick that comes from inside an agent run" in out


@pytest.mark.unit
@pytest.mark.parametrize("kind", ["review", "test"])
def test_review_and_test_get_lifecycle_workpad_repo_and_blocking(kind):
    """The sections review/test cross-reference must actually be in their
    prompt: no dangling "Blocking the run", the inlined workpad, the repo /
    PR block, and the shared lifecycle."""
    ctx = _ctx(kind)
    out = compose(kind, workspace=None, project=None, user=None, context=ctx).text
    assert "## Blocking the run" in out
    assert "## Task lifecycle" in out
    assert "## Workpad — read first, write last" in out
    assert ctx["workpad_body"] in out
    assert "Repository:" in out
    assert "### Path to done" in out


@pytest.mark.unit
@pytest.mark.parametrize("kind", ["review", "test"])
def test_review_and_test_do_not_get_the_implementation_workpad_checklist(kind):
    ctx = _ctx(kind)
    out = compose(kind, workspace=None, project=None, user=None, context=ctx).text
    assert "`### Progress Checkpoints` match what is actually true" not in out
    assert "carried forward **unchanged**" in out
    assert '"Analyze & scope" for tone' not in out


@pytest.mark.unit
def test_test_kind_defects_go_back_to_in_progress_not_blocked():
    out = compose("test", workspace=None, project=None, user=None, context=_ctx("test")).text
    assert "back to In Progress" in out
    assert "Blocked for a bug" in out


@pytest.mark.unit
def test_session_framing_comment_trigger_guidance():
    ctx = _ctx("coding-task")
    ctx["run"]["trigger"] = "comment_and_run"
    ctx["tick"] = None
    out = compose(
        "coding-task", workspace=None, project=None, user=None, context=ctx
    ).text
    assert "a new human comment" in out
    assert "automatically by the issue's ticker" not in out
    assert "Ticking schedule" not in out


@pytest.mark.unit
def test_session_framing_omits_trigger_block_for_scheduler():
    # The scheduler context has no run.trigger / tick keys; the shared
    # section must render (StrictUndefined-safe) and skip the block.
    out = compose(
        "scheduler", workspace=None, project=None, user=None, context=_ctx("scheduler")
    ).text
    assert "Why this run started" not in out
    assert "Ticking schedule" not in out


# ----------------------------------------------------------------------
# Ancestor-chain required reading + parent-readiness (PDASHOSS01-97)
# ----------------------------------------------------------------------

REQUIRED_READING_DIRECTIVE = "Required reading before you implement:"


def _coding_ctx_chain(depth: int) -> dict:
    """A populated coding-task context whose ancestor chain has ``depth``
    issues (current + ancestors).

    ``depth == 1`` is parentless (``parent``/``lineage`` both None). ``depth
    == 2`` has a direct parent only — ``build_context`` leaves ``lineage``
    None for a 2-chain. ``depth >= 3`` additionally carries a multi-level
    ``lineage`` (grandparent+), current-first up to the root.
    """
    if depth < 1:
        raise ValueError("depth must be >= 1")
    if depth == 1:
        ctx = copy.deepcopy(sample_contexts("coding-task")[1])  # minimal: parentless
        assert ctx["parent"] is None and ctx["lineage"] is None
        return ctx
    ctx = copy.deepcopy(sample_contexts("coding-task")[0])  # populated: has a parent
    if depth == 2:
        ctx["lineage"] = None
        return ctx
    lineage = [
        {"identifier": "SAMPLE-1", "title": "Sample issue title"},
        {"identifier": "SAMPLE-0", "title": "Parent issue"},
    ]
    for i in range(depth - 3):
        lineage.append({"identifier": f"SAMPLE-mid{i}", "title": f"Ancestor {i}"})
    lineage.append({"identifier": "SAMPLE-root", "title": "Root issue"})
    ctx["lineage"] = lineage
    return ctx


@pytest.mark.unit
@pytest.mark.parametrize("depth", [2, 3, 4, 6])
def test_coding_task_requires_ancestor_reading_when_parent(depth):
    """When the issue has a parent (chain length >= 2), the assembled coding
    prompt must direct the agent to read the ancestor chain before it
    implements — required, not optional (PDASHOSS01-97). Holds whether the
    chain is just the parent (len 2, lineage None) or a multi-level lineage
    (len 3, 4, ...)."""
    ctx = _coding_ctx_chain(depth)
    body = compose("coding-task", workspace=None, project=None, user=None, context=ctx).text

    assert REQUIRED_READING_DIRECTIVE in body
    # analyze-and-scope step 2 walks the chain and assesses readiness.
    assert "Walk the ancestor chain to the root" in body
    assert "ready to implement against" in body
    # The old optional wording is gone.
    assert "To learn about any ancestor" not in body


@pytest.mark.unit
def test_coding_task_no_ancestor_directive_when_parentless():
    """A parentless issue gets no ancestor-chain directive — there is no chain
    to walk (PDASHOSS01-97)."""
    ctx = _coding_ctx_chain(1)
    body = compose("coding-task", workspace=None, project=None, user=None, context=ctx).text

    assert REQUIRED_READING_DIRECTIVE not in body
    assert "Walk the ancestor chain to the root" not in body


@pytest.mark.unit
def test_coding_task_ancestor_directive_adapts_to_chain_depth():
    """For a 2-chain the directive says the chain is just the parent; for a
    3+-chain it names walking up to the root and surfaces the root id."""
    body2 = compose(
        "coding-task", workspace=None, project=None, user=None, context=_coding_ctx_chain(2)
    ).text
    body3 = compose(
        "coding-task", workspace=None, project=None, user=None, context=_coding_ctx_chain(3)
    ).text

    assert "the chain here is just the parent" in body2
    assert "up to the root issue" not in body2  # no multi-level lineage for a 2-chain
    assert "up to the root issue" in body3
    assert "SAMPLE-root" in body3  # root id surfaced so the agent can walk to it


@pytest.mark.unit
def test_coding_task_parent_no_branch_routes_through_readiness_not_autofallback():
    """workpad-setup must route the 'parent has no implementation branch' case
    through the readiness judgment (research/design parent -> project base;
    in-progress implementation dependency -> block) rather than an automatic
    fall-back to the project base (PDASHOSS01-97)."""
    ctx = _coding_ctx_chain(2)
    ctx["repo"]["work_branch"] = None  # this issue has no branch yet -> resolve a base
    ctx["parent"]["work_branch"] = None  # parent has no implementation branch yet
    body = compose("coding-task", workspace=None, project=None, user=None, context=ctx).text

    assert "Do not treat this as an automatic fall-back to the project base" in body
    # The dependency case routes into the existing blocking flow by reference.
    assert 'Treat it as a blocker — follow "Blocking the run" instead of creating a branch' in body


# ----------------------------------------------------------------------
# Multi-part plan: finish a multi-part issue in one run (PDASHOSS01-168)
# ----------------------------------------------------------------------


@pytest.mark.unit
def test_coding_task_split_gate_offers_multipart_plan():
    """A large-but-clear issue must have a *Proceed with a multi-part plan*
    outcome that separates size from ambiguity, rather than being forced to
    block. The analyze-and-scope split gate used to allow Proceed only when the
    work "fits one reasonable unit of delivery", so any big issue cost a run and
    a human round-trip before any code (PDASHOSS01-168)."""
    body = compose(
        "coding-task", workspace=None, project=None, user=None, context=_ctx()
    ).text

    # The new, non-blocking outcome exists and is keyed on clarity, not size.
    assert "Proceed with a multi-part plan" in body
    assert "larger than one PR, but the requirements and design are clear" in body
    # Size and ambiguity are explicitly separated.
    assert "Size is a different axis from ambiguity." in body
    # Clarify is reserved for an open decision, not for a large-but-clear issue.
    assert "a real product, UX, scope, or interface decision is unanswered" in body
    # Split is reserved for genuinely separate deliverables in different issues.
    assert "independent deliverables that belong in **different issues**" in body


@pytest.mark.unit
def test_coding_task_softens_splitting_bias():
    """Once a plan is recorded or approved, the prompt must tell the agent not
    to keep slicing the work into ever-smaller pieces on its own — the old
    cost-comparison wording biased toward smaller pieces (slice 1 -> 1a/1b),
    PDASHOSS01-168."""
    body = compose(
        "coding-task", workspace=None, project=None, user=None, context=_ctx()
    ).text

    assert "don't split its parts further unless a new product or design decision actually surfaces" in body


@pytest.mark.unit
def test_coding_task_keeps_building_parts_after_a_pr():
    """After a part's PR is open with parts remaining, the implementation
    section must tell the agent to continue in the same run and must forbid
    advancing the stage / yielding done on a partial implementation
    (PDASHOSS01-168)."""
    body = compose(
        "coding-task", workspace=None, project=None, user=None, context=_ctx()
    ).text

    # Steps 5-6 loop over the plan's parts within one run.
    assert "you do **not** stop after the first" in body
    assert "go back to step 5 for the next part" in body
    # A partial implementation must not advance to In Review / yield done.
    assert "do not yield `done` while parts are still unbuilt" in body
    assert "a partial implementation must not move the issue to In Review" in body


# ----------------------------------------------------------------------
# Split into child issues (task-level), not per-run slices (PDASHOSS01-169)
# ----------------------------------------------------------------------


@pytest.mark.unit
def test_coding_task_split_standard_is_independence_not_size():
    """The split decision must be keyed on independence, not on size or on
    crossing layers. A modern agent can finish a large, clear change in one long
    run (there is no run timeout), and splitting one feature by layer lets each
    side build against its own guess of the shared contract — which is how voice
    dictation (PDASHOSS01-148) shipped with every part green and the mic broken.
    """
    body = compose(
        "coding-task", workspace=None, project=None, user=None, context=_ctx()
    ).text

    assert "Split on independence, never on size." in body
    # The old size and layer signals must be gone.
    assert "clearly more than one run's work" not in body
    assert "span different areas" not in body
    # One-line test the agent can apply, plus explicit split / do-not-split lists.
    assert "without ever seeing child A's code or decisions" in body
    assert "**Split when**" in body
    assert "**Do not split when** the parts share an interface that is not yet fixed" in body
    # Size is a fallback only; splitting has a real run cost.
    assert "**Size is only a fallback.**" in body
    assert "**Count the cost.**" in body
    # If a split must cross an interface: contract first, producer before consumer.
    assert "**Pin any shared contract first.**" in body
    assert "producer lands before the consumer starts" in body


@pytest.mark.unit
def test_coding_task_split_gate_creates_child_issues():
    """The split outcome must direct the agent to create child issues *itself*
    with `pidash issue create --parent`, list existing children first for
    idempotency, apply the guardrails, and park the parent — not just propose a
    split and leave triage to a human (PDASHOSS01-169)."""
    body = compose(
        "coding-task", workspace=None, project=None, user=None, context=_ctx()
    ).text

    # The outcome is now "create children yourself", keyed on genuinely separate tasks.
    assert "Split into child issues" in body
    assert "break it into child issues **yourself**" in body
    # Concrete CLI: create children under the parent, list them first for idempotency.
    assert "pidash issue create --project SAMPLE --parent SAMPLE-1" in body
    assert "pidash issue list --project SAMPLE --parent SAMPLE-1" in body
    # Guardrails: cap, single-run sizing, depth 1.
    assert "At most ~6 children per split" in body
    assert "do **not** split a child further (depth 1)" in body
    # Parent is parked (not left In Progress on waiting_on_external, which keeps ticking).
    assert "Move the parent to **Todo**" in body
    assert "keeps ticking for In Progress and would burn the parent's budget" in body
    # The child list goes into the parent's description: that is the signal a child's
    # run reads to recognise a tracking parent (the child only ever sees the parent's
    # description, never its comments).
    assert "Record the children in this parent's description" in body
    assert "pidash issue patch SAMPLE-1 --description" in body
    # The multi-part outcome (PDASHOSS01-168) is still the home for parts of one task.
    assert "Proceed with a multi-part plan" in body


@pytest.mark.unit
def test_coding_task_tracking_parent_is_context_not_blocker():
    """A child whose parent is a tracking issue (split into children, no branch
    of its own) must not hit the 'parent in progress with no branch' blocker —
    the readiness block treats a tracking parent as context and the base-branch
    resolution bases off the project base or a sibling branch (PDASHOSS01-169)."""
    ctx = _coding_ctx_chain(2)
    ctx["repo"]["work_branch"] = None  # this child has no branch yet -> resolve a base
    ctx["parent"]["work_branch"] = None  # tracking parent will never have a branch
    body = compose("coding-task", workspace=None, project=None, user=None, context=ctx).text

    # Readiness block (analyze-and-scope step 2): tracking parent is context, not a blocker.
    assert "Parent is a **tracking issue**" in body
    assert "context, not a blocker" in body
    # Base-branch resolution (workpad-setup): tracking parent -> project base or sibling branch.
    assert "it carries no code branch of its own and will never get one" in body


@pytest.mark.unit
def test_coding_task_advances_stage_only_when_all_parts_done():
    """The issue moves to In Review only when every planned part is built; the
    In Progress exit condition and the ending routing both carry the
    all-parts-done gate (PDASHOSS01-168)."""
    body = compose(
        "coding-task", workspace=None, project=None, user=None, context=_ctx()
    ).text

    # task-lifecycle exit condition now requires every part built.
    assert "Multi-part issues stay In Progress until the whole issue is done." in body
    # While parts remain the run reports progressed (or waiting_on_external),
    # not done, and stays In Progress.
    assert "the issue **stays In Progress**" in body
    assert "Plan parts still remain" in body
    # The whole-issue testing hand-off is posted once, listing every PR.
    assert "Hand off to testing — once, for the whole issue, when every plan part is built." in body
    assert "list every PR" in body


# ----------------------------------------------------------------------
# Work item relationships section (PDASHOSS01-160): one independent section
# carrying ancestors, children, and relates_to siblings together.
# ----------------------------------------------------------------------

RELATIONSHIPS_HEADING = "## Work item relationships"


def _relationships_ctx(*, parent=True, children=0, related=0) -> dict:
    """A coding-task context with the relationship groups dialled independently.

    Starts from the parentless minimal sample (so ``parent``/``lineage`` are
    None and both list groups start empty), then adds back exactly the groups
    the test wants.
    """
    ctx = copy.deepcopy(sample_contexts("coding-task")[1])
    assert ctx["parent"] is None and ctx["children"] == [] and ctx["related"] == []
    if parent:
        populated = sample_contexts("coding-task")[0]
        ctx["parent"] = copy.deepcopy(populated["parent"])
        ctx["lineage"] = None  # direct parent only
    ctx["children"] = [
        {"identifier": f"SAMPLE-c{i}", "title": f"Child {i}", "state": "Backlog"}
        for i in range(children)
    ]
    ctx["related"] = [
        {"identifier": f"SAMPLE-r{i}", "title": f"Related {i}", "state": "Cancelled"}
        for i in range(related)
    ]
    return ctx


@pytest.mark.unit
def test_relationships_section_absent_when_nothing_connected():
    """No parent, no children, no relations → no section at all: no heading,
    no dangling required-reading directive."""
    ctx = _relationships_ctx(parent=False, children=0, related=0)
    body = compose("coding-task", workspace=None, project=None, user=None, context=ctx).text

    assert RELATIONSHIPS_HEADING not in body
    assert REQUIRED_READING_DIRECTIVE not in body


@pytest.mark.unit
def test_relationships_section_renders_children_group():
    """A child-only issue (no parent, no relations) renders just the children
    group under the single relationships section."""
    ctx = _relationships_ctx(parent=False, children=2, related=0)
    body = compose("coding-task", workspace=None, project=None, user=None, context=ctx).text

    assert RELATIONSHIPS_HEADING in body
    assert "Children (down):" in body
    assert "SAMPLE-c0: Child 0 (Backlog)" in body
    assert "SAMPLE-c1: Child 1 (Backlog)" in body
    assert REQUIRED_READING_DIRECTIVE in body
    # Groups degrade independently: no parent / related content leaks in.
    assert "Ancestors (up):" not in body
    assert "Related work items (across):" not in body


@pytest.mark.unit
def test_relationships_section_renders_related_group():
    """A relates_to-only issue renders just the related group."""
    ctx = _relationships_ctx(parent=False, children=0, related=1)
    body = compose("coding-task", workspace=None, project=None, user=None, context=ctx).text

    assert RELATIONSHIPS_HEADING in body
    assert "Related work items (across):" in body
    assert "SAMPLE-r0: Related 0 (Cancelled)" in body
    assert "Children (down):" not in body
    assert "Ancestors (up):" not in body


@pytest.mark.unit
def test_relationships_section_renders_all_three_groups_together():
    """Ancestors, children, and related render in one contiguous section with a
    single required-reading directive."""
    ctx = _relationships_ctx(parent=True, children=1, related=1)
    body = compose("coding-task", workspace=None, project=None, user=None, context=ctx).text

    assert body.count(RELATIONSHIPS_HEADING) == 1
    assert "Ancestors (up):" in body
    assert "Children (down):" in body
    assert "Related work items (across):" in body
    # Parent keeps its inline description + comment-count hint.
    assert "Parent description." in body
    assert "run `pidash comment list SAMPLE-0` to read them." in body
    # Exactly one required-reading directive, not one per group.
    assert body.count(REQUIRED_READING_DIRECTIVE) == 1


@pytest.mark.unit
def test_relationships_section_lineage_only_when_grandparent():
    """The lineage chain renders only when a grandparent+ exists; a direct-parent
    issue shows the parent line but no lineage chain."""
    ctx = _relationships_ctx(parent=True, children=0, related=0)
    ctx["lineage"] = None
    body2 = compose("coding-task", workspace=None, project=None, user=None, context=ctx).text
    assert "Lineage (current → root):" not in body2

    ctx["lineage"] = [
        {"identifier": "SAMPLE-1", "title": "Current"},
        {"identifier": "SAMPLE-0", "title": "Parent issue"},
        {"identifier": "SAMPLE-root", "title": "Root issue"},
    ]
    body3 = compose("coding-task", workspace=None, project=None, user=None, context=ctx).text
    assert "Lineage (current → root):" in body3
    assert "SAMPLE-root" in body3
