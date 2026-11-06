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
from django.core.checks import Error, Warning, register

from pi_dash.core.agent_execution import AgentExecutorKind


@register()
def managed_runner_configuration_check(app_configs, **kwargs):
    errors = []
    default = settings.DEFAULT_AGENT_EXECUTOR
    # Runner is the only executor for new projects (built_engine_as_runner
    # §6). Another value is ignored rather than refused at boot, so an
    # instance configured for the old choices keeps starting.
    if default != AgentExecutorKind.LOCAL_RUNNER:
        errors.append(
            Warning(
                f"DEFAULT_AGENT_EXECUTOR={default!r} is ignored: new projects always use local_runner",
                id="managed.W001",
            )
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
