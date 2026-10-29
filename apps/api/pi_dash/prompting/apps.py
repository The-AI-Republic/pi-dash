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
        from pi_dash.prompting import recipes, registry, work_types

        def _check_expanded(table_name: str, kind: str, entries) -> None:
            # Slots must name known slot positions, and every (kind × work
            # type) expansion must reference only real sections.
            for entry in entries:
                if isinstance(entry, recipes.Slot):
                    if entry.name not in work_types.SLOT_NAMES:
                        raise registry.PromptRegistryError(
                            f"{table_name} recipe {kind!r} has unknown slot {entry.name!r}"
                        )
            for work_type in work_types.WORK_TYPES:
                for key in work_types.expand(entries, work_type):
                    if key not in registry.REGISTRY:
                        raise registry.PromptRegistryError(
                            f"{table_name} recipe {kind!r} (work type {work_type!r}) references unknown section {key!r}"
                        )

        for kind, entries in recipes.RECIPES.items():
            _check_expanded("local", kind, entries)
        for kind, entries in recipes.MANAGED_RECIPES.items():
            _check_expanded("managed", kind, entries)
        for kind, section_keys in recipes.CLOUD_RECIPES.items():
            for key in section_keys:
                section = registry.REGISTRY.get(key)
                if section is None:
                    raise registry.PromptRegistryError(f"Cloud recipe {kind!r} references unknown section {key!r}")
                if section.customizable != registry.CUSTOMIZABLE_LOCKED:
                    raise registry.PromptRegistryError(f"Cloud recipe section {key!r} must be locked")
        # Every ticking phase must map to a real recipe — in BOTH executors'
        # maps, since any phase can fire on a cloud-executor project — else a
        # run would only fail at creation time with a confusing RecipeNotFound.
        for cfg in PHASES.values():
            if cfg.template_name not in recipes.RECIPES:
                raise registry.PromptRegistryError(
                    f"phase {cfg.state_name!r} maps to unknown recipe {cfg.template_name!r}"
                )
            if cfg.template_name not in recipes.CLOUD_RECIPES:
                raise registry.PromptRegistryError(
                    f"phase {cfg.state_name!r} has no Cloud Agent recipe for {cfg.template_name!r}"
                )
            if cfg.template_name not in recipes.MANAGED_RECIPES:
                raise registry.PromptRegistryError(
                    f"phase {cfg.state_name!r} has no managed-runner recipe for {cfg.template_name!r}"
                )
