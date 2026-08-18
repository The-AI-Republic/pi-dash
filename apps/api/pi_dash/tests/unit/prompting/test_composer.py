# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Composer: assembly, override resolution, manifest, and error attribution."""

from __future__ import annotations

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

    # The review success path routes to In Review...
    assert '--state "In Review"' in body
    # ...and never to Done.
    assert '--state "Done"' not in body
    # The approved outcome explicitly leaves the issue In Review.
    assert "leave the issue In Review" in body or "leaves the issue In Review" in body


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
    assert "about every 3 hours" in out
    assert "used 5 of 24 ticks" in out
    assert "19 remaining before the issue auto-pauses" in out


@pytest.mark.unit
def test_session_framing_review_tick_adds_noop_hint():
    ctx = _ctx("review")
    out = compose(
        "review", workspace=None, project=None, user=None, context=ctx
    ).text
    assert "automatically by the issue's ticker" in out
    assert "emit `noop`" in out  # review-specific done-signal nudge


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
