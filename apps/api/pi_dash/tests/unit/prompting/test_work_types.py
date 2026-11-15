# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Work-type axis: loader, slot expansion, core-section neutrality, and the
(stage × work type) compose/render matrix (PDASHOSS01-234)."""

from __future__ import annotations

import copy
from types import SimpleNamespace

import pytest

from pi_dash.prompting import neutrality, recipes, registry, work_types
from pi_dash.prompting.composer import compose
from pi_dash.prompting.validation import sample_contexts

#: The three issue-stage kinds whose recipes carry work-type slots.
STAGE_KINDS = neutrality.STAGE_KINDS

def _neutrality_violations(text: str) -> list[str]:
    return neutrality.violations(text)


# ----------------------------------------------------------------------
# Loader
# ----------------------------------------------------------------------


@pytest.mark.unit
def test_software_work_type_loaded_with_all_slots():
    wt = work_types.get_work_type("software")
    assert wt.key == "software"
    assert wt.title == "Software"
    assert wt.description
    assert set(wt.sections) == set(work_types.SLOT_NAMES)
    assert wt.sections["execute"] == "software.execute"
    # Every slot section is a real registry section with the namespaced key.
    for section_key in wt.sections.values():
        assert registry.get_section(section_key).key == section_key


@pytest.mark.unit
def test_default_work_type_exists():
    assert work_types.DEFAULT_WORK_TYPE in work_types.WORK_TYPES


@pytest.mark.unit
def test_get_work_type_unknown_raises():
    with pytest.raises(work_types.WorkTypeError):
        work_types.get_work_type("nope")


# ----------------------------------------------------------------------
# Slot expansion
# ----------------------------------------------------------------------


@pytest.mark.unit
def test_expand_fills_stage_slots_for_software():
    expanded = work_types.expand(recipes.recipe_for(recipes.KIND_CODING_TASK), "software")
    assert "software.context" in expanded
    assert "software.execute" in expanded
    # Order: context right after intro; execute between workpad-setup and
    # implementation — the positions the moved text occupied.
    assert expanded.index("software.context") == expanded.index("intro") + 1
    assert expanded.index("workpad-setup") < expanded.index("software.execute") < expanded.index("implementation")

    review = work_types.expand(recipes.recipe_for(recipes.KIND_REVIEW), "software")
    assert review.index("review-cycle") < review.index("software.review") < review.index("blocking")
    test = work_types.expand(recipes.recipe_for(recipes.KIND_TEST), "software")
    assert test.index("test-cycle") < test.index("software.test") < test.index("blocking")


@pytest.mark.unit
def test_expand_is_identity_for_slotless_recipes():
    recipe = recipes.recipe_for(recipes.KIND_SCHEDULER)
    assert work_types.expand(recipe, "software") == tuple(recipe)


@pytest.mark.unit
def test_expand_unknown_work_type_raises():
    with pytest.raises(work_types.WorkTypeError):
        work_types.expand(recipes.recipe_for(recipes.KIND_CODING_TASK), "nope")


@pytest.mark.unit
def test_expanded_recipes_contain_no_slots_and_only_real_sections():
    for kind in recipes.all_kinds():
        for wt in work_types.WORK_TYPES:
            for key in work_types.expand(recipes.recipe_for(kind), wt):
                assert isinstance(key, str)
                assert key in registry.REGISTRY


@pytest.mark.unit
def test_missing_slot_falls_back_to_general_when_available():
    # A synthetic work type with no sections: every slot resolves via the
    # ``general`` fallback (or empty while ``general`` does not exist yet).
    bare = work_types.WorkType(key="bare", title="Bare", description="d", sections={})
    general = work_types.WORK_TYPES.get(work_types.WORK_TYPE_GENERAL)
    for slot in work_types.SLOT_NAMES:
        expected = general.sections.get(slot) if general is not None else None
        assert work_types.section_key_for_slot(bare, slot) == expected


# ----------------------------------------------------------------------
# Effective work type resolution
# ----------------------------------------------------------------------


@pytest.mark.unit
def test_effective_work_type_defaults_and_overrides():
    # No fields anywhere → the ``general`` safety net.
    assert work_types.effective_work_type(SimpleNamespace(project=None)) == work_types.WORK_TYPE_GENERAL
    # Issue-level value wins.
    issue = SimpleNamespace(work_type="software", project=SimpleNamespace(default_work_type="general"))
    assert work_types.effective_work_type(issue) == "software"
    # Project default applies when the issue has none.
    issue = SimpleNamespace(work_type=None, project=SimpleNamespace(default_work_type="software"))
    assert work_types.effective_work_type(issue) == "software"
    # Unknown stored keys fall back to the default rather than failing a run.
    issue = SimpleNamespace(work_type="not-a-work-type", project=None)
    assert work_types.effective_work_type(issue) == work_types.DEFAULT_WORK_TYPE
    # An unknown issue-level key is treated as absent: the project default
    # still applies rather than being skipped for the global default.
    issue = SimpleNamespace(work_type="not-a-work-type", project=SimpleNamespace(default_work_type="software"))
    assert work_types.effective_work_type(issue) == "software"


# ----------------------------------------------------------------------
# Core-section neutrality (acceptance criterion: enforced by a unit test)
# ----------------------------------------------------------------------


@pytest.mark.unit
def test_core_stage_sections_are_work_type_neutral():
    """Core sections of the three stage recipes must not carry git/PR-specific
    wording; only work-type sections may."""
    core_keys = set()
    for kind in STAGE_KINDS:
        for entry in recipes.recipe_for(kind):
            if isinstance(entry, str):
                core_keys.add(entry)
    violations = {}
    for key in sorted(core_keys):
        hits = _neutrality_violations(registry.get_section(key).default_body)
        if hits:
            violations[key] = hits
    assert not violations, f"core sections carry work-type-specific wording: {violations}"


@pytest.mark.unit
def test_non_software_prompts_render_without_git_wording():
    """For every non-software work type, none of the three stage prompts may
    mention git, branches, or PRs (acceptance criterion for ``general``)."""
    for wt in work_types.WORK_TYPES:
        if wt == work_types.WORK_TYPE_SOFTWARE:
            continue
        for kind in STAGE_KINDS:
            for ctx in sample_contexts(kind):
                out = compose(kind, workspace=None, project=None, user=None, context=ctx, work_type=wt)
                hits = _neutrality_violations(out.text)
                assert not hits, f"{kind} prompt for work type {wt!r} leaks software wording: {hits}"


# ----------------------------------------------------------------------
# (stage × work type) compose/render matrix
# ----------------------------------------------------------------------


@pytest.mark.unit
def test_every_stage_work_type_combination_composes_and_renders():
    for kind in recipes.all_kinds():
        for wt in work_types.WORK_TYPES:
            for ctx in sample_contexts(kind):
                out = compose(kind, workspace=None, project=None, user=None, context=ctx, work_type=wt)
                assert out.text.strip()
                assert "{%" not in out.text and "{{" not in out.text
                # The manifest records exactly the expanded recipe, so the
                # work-type sections used are visible per run.
                expected = list(work_types.expand(recipes.recipe_for(kind), wt))
                assert [e.section_key for e in out.manifest] == expected


@pytest.mark.unit
def test_software_prompts_keep_todays_coding_guidance():
    """A project on ``software`` still gets the tuned git/PR instructions in
    all three stages (acceptance criterion)."""
    populated = {kind: sample_contexts(kind)[0] for kind in STAGE_KINDS}
    # No pinned work branch and no parent → the fresh-branch-off-base path.
    execute_ctx = copy.deepcopy(populated[recipes.KIND_CODING_TASK])
    execute_ctx["repo"]["work_branch"] = None
    execute_ctx["parent"] = None
    execute_ctx["lineage"] = None

    execute = compose(
        recipes.KIND_CODING_TASK,
        workspace=None,
        project=None,
        user=None,
        context=execute_ctx,
        work_type="software",
    ).text
    # Fresh branch off the latest base, commit/push, open + attach the review.
    assert 'git checkout "$BASE" && git pull --rebase origin "$BASE"' in execute
    assert "git checkout -b" in execute
    assert "git push -u origin" in execute
    assert "pidash issue attach-review" in execute
    assert "CLAUDE.md" in execute

    review = compose(
        recipes.KIND_REVIEW,
        workspace=None,
        project=None,
        user=None,
        context=populated[recipes.KIND_REVIEW],
        work_type="software",
    ).text
    # PR review guidance (the populated sample repo is GitHub).
    assert "DESIGN_THEN_CODE" in review
    assert "`gh` CLI" in review

    test = compose(
        recipes.KIND_TEST,
        workspace=None,
        project=None,
        user=None,
        context=populated[recipes.KIND_TEST],
        work_type="software",
    ).text
    # Repo-gate discovery and running the app.
    assert ".github/workflows/" in test
    assert "somewhere the software runs" in test
    assert "AUTOMATED" in test and "OPS / INFRA" in test


# ----------------------------------------------------------------------
# Existing overrides of core sections (revalidate_section_overrides)
# ----------------------------------------------------------------------

_LEGACY_IMPLEMENTATION_OVERRIDE = (
    "## Step 2\n1. Commit and push the branch, then open a PR with `gh pr create`.\n"
)


@pytest.mark.unit
def test_override_neutrality_flags_core_section_with_git_wording():
    detail = neutrality.override_neutrality_issue("implementation", _LEGACY_IMPLEMENTATION_OVERRIDE)
    assert detail == (
        "override for core section 'implementation' carries work-type-specific wording "
        "('branch', 'Commit', 'push', 'gh', 'PR'); core sections are shared by every work type, "
        "so move that guidance into a work-type section (e.g. 'software.execute')"
    )


@pytest.mark.unit
def test_override_neutrality_allows_neutral_core_and_any_work_type_section():
    assert neutrality.override_neutrality_issue("implementation", "Deliver the part and record it.") is None
    # Software wording is exactly what work-type sections are for.
    assert neutrality.override_neutrality_issue("software.execute", _LEGACY_IMPLEMENTATION_OVERRIDE) is None
    # Scheduler-only sections are not composed with a work type.
    assert "scheduler-task" not in neutrality.core_stage_section_keys()
    assert neutrality.override_neutrality_issue("scheduler-task", _LEGACY_IMPLEMENTATION_OVERRIDE) is None


@pytest.mark.unit
def test_revalidate_flags_core_override_with_git_wording(db, workspace, create_user):
    from io import StringIO

    from django.core.management import call_command

    from pi_dash.prompting.models import PromptSectionOverride

    legacy = PromptSectionOverride.objects.create(
        workspace=workspace, user=None, section_key="implementation", body=_LEGACY_IMPLEMENTATION_OVERRIDE
    )
    neutral = PromptSectionOverride.objects.create(
        workspace=workspace, user=None, section_key="default-posture", body="Work autonomously; ask only when blocked."
    )
    software = PromptSectionOverride.objects.create(
        workspace=workspace, user=None, section_key="software.execute", body=_LEGACY_IMPLEMENTATION_OVERRIDE
    )

    out = StringIO()
    call_command("revalidate_section_overrides", stdout=out)

    for row in (legacy, neutral, software):
        row.refresh_from_db()
    assert legacy.needs_attention is True
    assert neutral.needs_attention is False
    assert software.needs_attention is False
    assert "carries work-type-specific wording" in out.getvalue()
    assert "checked 3 active override(s): 1 newly flagged, 0 cleared." in out.getvalue()

    # --clear never clears a row that is still non-neutral.
    call_command("revalidate_section_overrides", "--clear", stdout=StringIO())
    legacy.refresh_from_db()
    assert legacy.needs_attention is True
