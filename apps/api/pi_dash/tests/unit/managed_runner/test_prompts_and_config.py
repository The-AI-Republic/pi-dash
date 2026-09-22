# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Prompt recipes, startup checks and settings for the managed runner.

The managed runner has the same capabilities as a local one, so it must get the
same prompt sections — but through its own map, so a future divergence is a
one-key override rather than a rewrite, and so the boot check can prove
completeness per executor.
"""

from __future__ import annotations

import pytest
from django.test import override_settings

from pi_dash.core.agent_execution import AgentExecutorKind, managed_runner_is_enabled
from pi_dash.prompting import recipes

from .conftest import MANAGED_SETTINGS

pytestmark = pytest.mark.unit


# --------------------------------------------------------------------------
# Recipes
# --------------------------------------------------------------------------


def test_managed_recipes_mirror_local_recipes():
    """An alias, not a fork: identical content today, separately addressable."""
    assert recipes.MANAGED_RECIPES == recipes.RECIPES
    assert recipes.MANAGED_RECIPES is not recipes.RECIPES


def test_managed_recipes_share_local_sections_unlike_cloud():
    """Cloud recipes deliberately share no local-runner section (no CLI, no
    filesystem). The managed runner has both, so it must not inherit that
    restriction."""
    coding = recipes.MANAGED_RECIPES[recipes.KIND_CODING_TASK]
    assert "pidash-cli" in coding
    assert not any(key.startswith("cloud-") for key in coding)


def test_recipe_for_routes_by_executor_kind():
    kind = recipes.KIND_CODING_TASK
    assert recipes.recipe_for(kind) == recipes.RECIPES[kind]
    assert (
        recipes.recipe_for(kind, executor_kind=AgentExecutorKind.MANAGED_RUNNER)
        == recipes.MANAGED_RECIPES[kind]
    )
    # An unknown executor falls back to local rather than raising: the caller
    # is describing where a run executes, not selecting a template, and a
    # machine executor is always the safe default.
    assert recipes.recipe_for(kind, executor_kind="something-else") == recipes.RECIPES[kind]


def test_recipe_for_raises_for_unknown_kind():
    with pytest.raises(recipes.RecipeNotFound):
        recipes.recipe_for("not-a-kind", executor_kind=AgentExecutorKind.MANAGED_RUNNER)


def test_every_phase_has_a_managed_recipe():
    """The boot check enforces this; assert it directly so a phase added
    without a managed recipe fails here with a clearer message than a
    startup traceback."""
    from pi_dash.orchestration.agent_phases import PHASES

    missing = [cfg.state_name for cfg in PHASES.values() if cfg.template_name not in recipes.MANAGED_RECIPES]
    assert missing == []


def test_section_usage_lookup_includes_managed_kinds():
    """The admin's "where is this section used" view must not under-report
    once a section is reachable through the managed map."""
    from pi_dash.prompting import validation

    section = recipes.MANAGED_RECIPES[recipes.KIND_CODING_TASK][0]
    kinds = validation.kinds_for_section(section)
    assert recipes.KIND_CODING_TASK in kinds
    # No duplicates: a section in both the local and managed maps must be
    # reported once, or the admin sees the same kind listed twice.
    assert len(kinds) == len(set(kinds))


# --------------------------------------------------------------------------
# Composition
# --------------------------------------------------------------------------


@pytest.mark.django_db
def test_compose_threads_executor_kind_to_recipe_selection(workspace, project, create_user, monkeypatch):
    """``compose`` must pass the run's executor through to recipe selection.

    Asserting on the selection rather than the rendered text keeps this test
    about the wiring: the two maps are equal today, so comparing output would
    pass even if the kwarg were dropped on the floor.
    """
    from pi_dash.prompting import composer

    seen: list = []
    real = recipes.recipe_for

    def spy(kind, *, executor_kind=None):
        seen.append(executor_kind)
        return real(kind, executor_kind=executor_kind)

    monkeypatch.setattr(composer.recipes, "recipe_for", spy)
    context = {"issue": {"name": "x"}, "project": {"name": "p"}}
    for executor in (None, AgentExecutorKind.MANAGED_RUNNER):
        try:
            composer.compose(
                recipes.KIND_CODING_TASK,
                workspace=workspace,
                project=project,
                user=create_user,
                context=context,
                executor_kind=executor,
            )
        except Exception:
            # Rendering needs a full issue context; selection already happened.
            pass
    assert seen == [None, AgentExecutorKind.MANAGED_RUNNER]


# --------------------------------------------------------------------------
# Settings and checks
# --------------------------------------------------------------------------


@override_settings(MANAGED_RUNNER_ENABLED=False)
def test_kill_switch_defaults_off():
    assert managed_runner_is_enabled() is False


@override_settings(**MANAGED_SETTINGS)
def test_kill_switch_reads_setting():
    assert managed_runner_is_enabled() is True


@override_settings(DEFAULT_AGENT_EXECUTOR="managed_runner", MANAGED_RUNNER_ENABLED=False)
def test_check_rejects_managed_default_without_the_switch():
    """An instance that defaults to an executor it can never admit would fail
    every run at creation; fail at boot instead."""
    from pi_dash.managed_runner.checks import managed_runner_configuration_check

    ids = [e.id for e in managed_runner_configuration_check(None)]
    assert "managed.E001" in ids


@override_settings(DEFAULT_AGENT_EXECUTOR="local_runner", **MANAGED_SETTINGS)
def test_check_passes_for_a_sane_configuration():
    from pi_dash.managed_runner.checks import managed_runner_configuration_check

    assert managed_runner_configuration_check(None) == []


@override_settings(MANAGED_RUNNER_MAX_PER_USER_PROJECT=0, MANAGED_RUNNER_QUEUED_MAX_AGE_SECS=0)
def test_check_rejects_nonsense_limits():
    from pi_dash.managed_runner.checks import managed_runner_configuration_check

    ids = {e.id for e in managed_runner_configuration_check(None)}
    assert {"managed.E003", "managed.E004"} <= ids
