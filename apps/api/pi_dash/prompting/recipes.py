# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Prompt recipes — ordered section lists per prompt kind.

A **recipe** names which sections compose a prompt *kind* and in what order.
Recipes are code-owned (not user-editable): section *content* is the
customization surface, section *order and membership* is not — order encodes
cross-references between sections.

Kind names align with ``PhaseConfig.template_name`` in
``orchestration/agent_phases.py``: the phase registry maps an issue's state to
a kind, and this module maps a kind to its section list.

Issue-stage recipes carry named :class:`Slot` entries alongside core section
keys. A slot is filled at compose time with the section the issue's **work
type** supplies for it (``work_types.expand``) — e.g. the ``execute`` slot of
the In Progress recipe resolves to ``software.execute`` for the ``software``
work type. Core sections stay work-type-neutral; everything git/PR-specific
lives in work-type sections.

See ``.ai_design/prompt_section_system/design.md`` §4 and §9.5 (the deferred
work-kind axis this supersedes — the axis ships as **work type**).
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Slot:
    """A named hole in a recipe, filled by the issue's work type.

    ``name`` must be one of ``work_types.SLOT_NAMES``. A work type that
    supplies no section for a slot simply leaves it empty (after the
    ``general`` fallback, see ``work_types.expand``).
    """

    name: str


SLOT_CONTEXT = Slot("context")
SLOT_EXECUTE = Slot("execute")
SLOT_REVIEW = Slot("review")
SLOT_TEST = Slot("test")

#: Prompt kinds. ``CODING_TASK`` / ``REVIEW`` mirror the legacy
#: ``PromptTemplate`` names so the phase registry keeps working unchanged;
#: ``SCHEDULER`` is the project-scoped kind unified onto the composer.
KIND_CODING_TASK = "coding-task"
KIND_REVIEW = "review"
KIND_TEST = "test"
KIND_SCHEDULER = "scheduler"
KIND_DIRECT = "direct"

RECIPES: dict[str, tuple] = {
    KIND_CODING_TASK: (
        "intro",
        SLOT_CONTEXT,
        "relationships",
        "session-framing",
        "pidash-cli",
        "task-lifecycle",
        "default-posture",
        "autonomy",
        "state-routing",
        "analyze-and-scope",
        "workpad-setup",
        SLOT_EXECUTE,
        "implementation",
        "blocking",
        "guardrails",
        "workpad-template",
        "ending-run",
    ),
    # Review and test share the lifecycle, the work-type context block, the
    # inlined workpad, and the blocking flow with the coding task — every
    # section their own text cross-references (design §8.2).
    KIND_REVIEW: (
        "review-intro",
        SLOT_CONTEXT,
        "session-framing",
        "pidash-cli",
        "task-lifecycle",
        "workpad-context",
        "review-cycle",
        SLOT_REVIEW,
        "blocking",
        "guardrails",
        "ending-run",
    ),
    KIND_TEST: (
        "test-intro",
        SLOT_CONTEXT,
        "session-framing",
        "pidash-cli",
        "task-lifecycle",
        "workpad-context",
        "test-cycle",
        SLOT_TEST,
        "blocking",
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

# Locked executor-owned recipes. They deliberately share no local Runner
# section, preventing CLI/filesystem instructions from entering Cloud prompts.
CLOUD_RECIPES: dict[str, tuple[str, ...]] = {
    KIND_CODING_TASK: (
        "cloud-intro",
        "cloud-capabilities",
        "cloud-issue-context",
        "cloud-execution-loop",
        "cloud-write-policy",
        "cloud-ending",
    ),
    KIND_REVIEW: (
        "cloud-review-intro",
        "cloud-capabilities",
        "cloud-issue-context",
        "cloud-review-loop",
        "cloud-write-policy",
        "cloud-ending",
    ),
    KIND_TEST: (
        "cloud-test-intro",
        "cloud-capabilities",
        "cloud-issue-context",
        "cloud-test-loop",
        "cloud-write-policy",
        "cloud-ending",
    ),
    KIND_SCHEDULER: (
        "cloud-scheduler-intro",
        "cloud-capabilities",
        "cloud-scheduler-task",
        "cloud-scheduler-loop",
        "cloud-write-policy",
        "cloud-ending",
    ),
    KIND_DIRECT: (
        "cloud-intro",
        "cloud-capabilities",
        "cloud-direct-task",
        "cloud-execution-loop",
        "cloud-write-policy",
        "cloud-ending",
    ),
}

#: Legacy name for the default work axis value. The §9.5 work-kind axis
#: shipped as **work type** (``prompting/work_types.py``): the stage keeps
#: selecting the kind/recipe unchanged, and the work type only fills the
#: recipe's slots at compose time. Kept so ``kind_for`` callers stay stable.
WORK_KIND_CODING = "coding"


class RecipeNotFound(Exception):
    """Raised when a kind has no registered recipe."""


def kind_for(template_name: str, work_kind: str = WORK_KIND_CODING) -> str:
    """Resolve a prompt *kind* from a phase template name.

    ``template_name`` comes from the phase registry
    (``agent_phases.template_name_for``). This is an identity on
    ``template_name`` and stays one: the work-type axis (design §9.5) landed
    as recipe slots (``work_types.expand``) rather than as extra kinds, so the
    kind — and everything stamped from it (``phase_kind``, ticking, the
    outcome guard) — depends only on the stage. ``work_kind`` is kept for
    signature compatibility and is ignored.
    """
    return template_name


#: Recipes for the desktop-bundled managed runner.
#:
#: An **alias** of the local map, not a copy: a managed runner has exactly the
#: same capabilities as a user-installed one (filesystem, shell, worktree, the
#: ``pidash`` CLI), so it needs exactly the same sections. Keeping it a separate
#: name means a future divergence is a one-key override here rather than a
#: rewrite of every caller — and the startup check can prove completeness for
#: all three executors independently.
MANAGED_RECIPES: dict[str, tuple] = dict(RECIPES)


def recipe_for(kind: str, *, executor_kind: str | None = None) -> tuple:
    """Recipe entries (section keys and slots) for ``kind`` on
    ``executor_kind`` (default: local runner).

    The returned tuple may contain :class:`Slot` entries; resolve them with
    ``work_types.expand(recipe, work_type)`` before composing.

    Cloud recipes are deliberately not reachable here — they are locked,
    share no local-runner section, and have their own accessor.
    """
    from pi_dash.core.agent_execution import AgentExecutorKind

    table = MANAGED_RECIPES if executor_kind == AgentExecutorKind.MANAGED_RUNNER else RECIPES
    try:
        return table[kind]
    except KeyError as exc:
        raise RecipeNotFound(f"no recipe for kind {kind!r}") from exc


def cloud_recipe_for(kind: str) -> tuple[str, ...]:
    try:
        return CLOUD_RECIPES[kind]
    except KeyError as exc:
        raise RecipeNotFound(f"no Cloud Agent recipe for kind {kind!r}") from exc


def all_kinds() -> tuple[str, ...]:
    return tuple(RECIPES.keys())
