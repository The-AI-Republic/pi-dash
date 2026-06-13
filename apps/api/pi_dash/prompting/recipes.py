# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Prompt recipes — ordered section lists per prompt kind.

A **recipe** names which sections compose a prompt *kind* and in what order.
Recipes are code-owned (not user-editable): section *content* is the
customization surface, section *order and membership* is not — order encodes
step numbering and cross-references between sections.

Kind names align with ``PhaseConfig.template_name`` in
``orchestration/agent_phases.py``: the phase registry maps an issue's state to
a kind, and this module maps a kind to its section list.

See ``.ai_design/prompt_section_system/design.md`` §4 and §9.5.
"""

from __future__ import annotations

#: Prompt kinds. ``CODING_TASK`` / ``REVIEW`` mirror the legacy
#: ``PromptTemplate`` names so the phase registry keeps working unchanged;
#: ``SCHEDULER`` is the project-scoped kind unified onto the composer.
KIND_CODING_TASK = "coding-task"
KIND_REVIEW = "review"
KIND_SCHEDULER = "scheduler"

RECIPES: dict[str, tuple[str, ...]] = {
    KIND_CODING_TASK: (
        "intro",
        "session-framing",
        "pidash-cli",
        "default-posture",
        "autonomy",
        "state-routing",
        "analyze-and-scope",
        "workpad-setup",
        "implementation",
        "blocking",
        "guardrails",
        "workpad-template",
        "ending-run",
    ),
    KIND_REVIEW: (
        "review-intro",
        "session-framing",
        "pidash-cli",
        "review-cycle",
        "guardrails",
        "ending-run",
    ),
    KIND_SCHEDULER: (
        "scheduler-intro",
        "session-framing",
        "pidash-cli",
        "scheduler-task",
        "guardrails",
        "scheduler-ending",
    ),
}

#: Default work kind. The work-kind axis (project default + per-issue override)
#: is designed-but-deferred (design §9.5); ``kind_for`` accepts ``work_kind``
#: from day one and hardcodes ``"coding"`` so the axis lands without touching
#: call sites.
WORK_KIND_CODING = "coding"


class RecipeNotFound(Exception):
    """Raised when a kind has no registered recipe."""


def kind_for(template_name: str, work_kind: str = WORK_KIND_CODING) -> str:
    """Resolve a prompt *kind* from a phase template name and a work kind.

    ``template_name`` comes from the phase registry
    (``agent_phases.template_name_for``); ``work_kind`` is the §9.5 seam,
    hardcoded to ``"coding"`` in v1. Today this is an identity on
    ``template_name`` — the work-kind matrix collapses to the coding row — but
    keeping the signature lets the matrix expand later (e.g.
    ``("In Progress", "ops") -> "ops-task"``) without changing callers.
    """
    return template_name


def recipe_for(kind: str) -> tuple[str, ...]:
    try:
        return RECIPES[kind]
    except KeyError as exc:
        raise RecipeNotFound(f"no recipe for kind {kind!r}") from exc


def all_kinds() -> tuple[str, ...]:
    return tuple(RECIPES.keys())
