# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Startup checks for the managed runner.

These exist to turn two silent misconfigurations into boot failures: an
instance whose default executor can never admit a run, and a phase with no
prompt recipe for the managed map (which would otherwise surface as a
confusing ``RecipeNotFound`` the first time someone clicks Run).
"""

from django.conf import settings
from django.core.checks import Error, register

from pi_dash.core.agent_execution import AgentExecutorKind


@register()
def managed_runner_configuration_check(app_configs, **kwargs):
    errors = []
    default = settings.DEFAULT_AGENT_EXECUTOR
    if default == AgentExecutorKind.MANAGED_RUNNER and not settings.MANAGED_RUNNER_ENABLED:
        errors.append(
            Error(
                "DEFAULT_AGENT_EXECUTOR=managed_runner requires MANAGED_RUNNER_ENABLED",
                id="managed.E001",
            )
        )
    if settings.MANAGED_RUNNER_MAX_PER_USER_PROJECT < 1:
        errors.append(
            Error("MANAGED_RUNNER_MAX_PER_USER_PROJECT must be at least 1", id="managed.E003")
        )
    if settings.MANAGED_RUNNER_QUEUED_MAX_AGE_SECS <= 0:
        errors.append(
            Error("MANAGED_RUNNER_QUEUED_MAX_AGE_SECS must be positive", id="managed.E004")
        )

    from pi_dash.orchestration.agent_phases import PHASES
    from pi_dash.prompting import recipes

    for cfg in PHASES.values():
        if cfg.template_name not in recipes.MANAGED_RECIPES:
            errors.append(
                Error(
                    f"phase {cfg.state_name!r} has no managed-runner recipe for {cfg.template_name!r}",
                    id="managed.E002",
                )
            )
    return errors
