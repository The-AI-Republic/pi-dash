# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Composer: assembly, override resolution, manifest, and error attribution."""

from __future__ import annotations

import pytest

from pi_dash.prompting import recipes
from pi_dash.prompting.composer import (
    SOURCE_DEFAULT,
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
@pytest.mark.parametrize("kind", ["coding-task", "review", "scheduler"])
def test_all_kinds_render_against_both_sample_contexts(kind):
    for ctx in sample_contexts(kind):
        out = compose(kind, workspace=None, project=None, user=None, context=ctx)
        assert out.text.strip()
        assert "{%" not in out.text and "{{" not in out.text


@pytest.mark.unit
def test_review_default_includes_cli_docs():
    out = compose("review", workspace=None, project=None, user=None, context=_ctx("review"))
    assert "Pi Dash CLI" in out.text
    assert "pidash workpad" in out.text


# ----------------------------------------------------------------------
# State-routing regression: PR work ends in `review`, not `completed`
# ----------------------------------------------------------------------


@pytest.mark.unit
def test_pr_work_routes_to_review_group_not_done():
    """A `code_change` that opens a PR must land in the `review` group
    ("In Review"), not `completed`/"Done".

    Regression for runs that marked PR-producing issues "Done": the prompt
    used to give ``--state "Done"`` as the *only* canonical success example,
    so a literal-minded agent (Codex) copied it and skipped In Review, while
    a judgement-driven agent (Claude) overrode it. The routing is now stated
    explicitly across pidash-cli / default-posture / implementation /
    ending-run so neither has to infer it.
    """
    out = compose(
        "coding-task", workspace=None, project=None, user=None, context=_ctx()
    )
    body = out.text

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
