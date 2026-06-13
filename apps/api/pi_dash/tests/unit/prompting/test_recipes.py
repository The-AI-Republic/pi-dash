# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Recipes and the kind-resolution seam."""

from __future__ import annotations

import pytest

from pi_dash.prompting import recipes


@pytest.mark.unit
def test_three_kinds_registered():
    assert set(recipes.all_kinds()) == {"coding-task", "review", "scheduler"}


@pytest.mark.unit
def test_kind_for_identity_on_template_name():
    # v1: work_kind hardcoded to "coding" → identity on the phase template name.
    assert recipes.kind_for("coding-task") == "coding-task"
    assert recipes.kind_for("review") == "review"
    assert recipes.kind_for("coding-task", "coding") == "coding-task"


@pytest.mark.unit
def test_recipe_for_returns_ordered_keys():
    coding = recipes.recipe_for("coding-task")
    assert coding[0] == "intro"
    assert coding[-1] == "ending-run"
    # shared sections appear in coding, review, and scheduler
    for shared in ("session-framing", "pidash-cli", "guardrails"):
        assert shared in recipes.recipe_for("coding-task")
        assert shared in recipes.recipe_for("review")
        assert shared in recipes.recipe_for("scheduler")


@pytest.mark.unit
def test_review_recipe_includes_cli_docs():
    # Regression guard for the original defect: review must carry the CLI docs.
    assert "pidash-cli" in recipes.recipe_for("review")


@pytest.mark.unit
def test_scheduler_recipe_has_task_section():
    assert "scheduler-task" in recipes.recipe_for("scheduler")


@pytest.mark.unit
def test_recipe_for_unknown_kind_raises():
    with pytest.raises(recipes.RecipeNotFound):
        recipes.recipe_for("nope")
