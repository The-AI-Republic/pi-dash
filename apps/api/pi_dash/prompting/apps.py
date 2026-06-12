# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.apps import AppConfig


class PromptingConfig(AppConfig):
    name = "pi_dash.prompting"
    label = "prompting"
    verbose_name = "Pi Dash Prompting"
    default_auto_field = "django.db.models.BigAutoField"

    def ready(self) -> None:
        # Prompt defaults are code (``prompting/sections/`` + ``recipes.py``),
        # not DB-seeded rows — so there is no post_migrate seed step. Instead,
        # validate at startup that every recipe references a real section, so a
        # bad recipe/section edit fails loudly here rather than at first render.
        # (The legacy ``PromptTemplate`` seed machinery in ``seed.py`` is kept
        # only for historical-migration replay; the table drop is deferred.)
        from pi_dash.orchestration.agent_phases import PHASES
        from pi_dash.prompting import recipes, registry

        for kind, section_keys in recipes.RECIPES.items():
            for key in section_keys:
                if key not in registry.REGISTRY:
                    raise registry.PromptRegistryError(
                        f"recipe {kind!r} references unknown section {key!r}"
                    )
        # Every ticking phase must map to a real recipe, else a run would only
        # fail at creation time with a confusing RecipeNotFound.
        for cfg in PHASES.values():
            if cfg.template_name not in recipes.RECIPES:
                raise registry.PromptRegistryError(
                    f"phase {cfg.state_name!r} maps to unknown recipe "
                    f"{cfg.template_name!r}"
                )
