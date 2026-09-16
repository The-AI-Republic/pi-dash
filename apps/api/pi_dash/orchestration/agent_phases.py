# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Phase registry for the issue ticking system.

Maps a state group to the (state name, prompt template, fresh-session flag)
that ticks in that group. Replaces the hard-coded ``"In Progress"`` literals
that previously lived in ``orchestration/service.py``,
``bgtasks/agent_ticker.py``, ``orchestration/scheduling.py`` and
``prompting/composer.py``.

See ``.ai_design/create_review_state/design.md`` §3 for the full design.

Cadence *values* (intervals) are intentionally **not** on ``PhaseConfig``.
They stay centrally managed on ``Project`` so an operator can retune a
phase's rhythm without a code deploy. What ``PhaseConfig`` carries is the
``cadence_key`` — which interval column a phase resolves through, via
``CADENCE_FIELDS``. Budget is *not* per phase: one pool per issue, spent
in any stage (``.ai_design/ticking_relevance/design.md`` §5).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

from pi_dash.db.models.state import StateGroup
from pi_dash.prompting.recipes import KIND_CODING_TASK


@dataclass(frozen=True)
class CadenceFields:
    """Which project column holds one phase's *interval*.

    Cadence is rhythm, not budget: each stage keeps its own interval (a
    test cycle is a slower loop than a review pass) but the **budget is one
    pool per issue** (``Project.agent_default_max_ticks`` +
    ``IssueAgentTicker.granted``), so there is no per-phase cap column any
    more. See ``.ai_design/ticking_relevance/design.md`` §5 / §9.
    """

    project_interval: str
    default_interval: int


#: Cadence key → the project interval column that phase reads. Keys are
#: opaque labels referenced by ``PhaseConfig.cadence_key``; they are
#: deliberately *not* state-group values, so a future phase can share a
#: column (or a group can be renamed) without touching the schema.
CADENCE_FIELDS: dict[str, CadenceFields] = {
    "impl": CadenceFields(
        project_interval="agent_default_interval_seconds",
        default_interval=43200,  # 12 h
    ),
    "review": CadenceFields(
        project_interval="agent_review_default_interval_seconds",
        default_interval=28800,  # 8 h
    ),
    "test": CadenceFields(
        project_interval="agent_test_default_interval_seconds",
        default_interval=43200,  # 12 h
    ),
}

#: Phases that are not registered ticking states (or states outside the
#: registry entirely) fall back to the implementation interval — the
#: pre-phase-split behavior.
DEFAULT_CADENCE_KEY = "impl"


@dataclass(frozen=True)
class PhaseConfig:
    """Static metadata for a ticking phase.

    Attributes:
        state_name:
            The literal state name that ticks in this group. Workspaces
            with bespoke state names within the group still don't tick in
            v1 — that is a separate generalization.
        template_name:
            The ``PromptTemplate.name`` to render on the phase's first
            run.
        cadence_key:
            Key into :data:`CADENCE_FIELDS` naming the project column this
            phase's interval resolves through.
        fresh_session_on_entry:
            When ``True``, entering this phase from a *different* ticking
            phase forces ``parent_run=None`` and clears
            ``pinned_runner_id`` so the template body becomes the actual
            system prompt rather than a user-turn message on a resumed
            session. See design §4.3.
        disarm_on_completed:
            When ``True``, a terminal ``completed``/``blocked``
            done-signal disarms the ticker for issues in this phase. v1
            sets ``True`` for every entry — kept here for explicitness.
        auto_pause_on_cap:
            When ``True``, exhausting the tick budget moves the issue to
            "Paused". Set ``False`` for the human-hand-off phases (In
            Review, In Test): the runner never promotes or reparks those
            on its own, so a cap-exhausted issue simply stays put for a
            human to act. Read by
            ``scheduling.maybe_apply_deferred_pause``; getting it wrong
            is silent — the issue is moved out from under the human.
    """

    state_name: str
    template_name: str
    cadence_key: str
    fresh_session_on_entry: bool
    disarm_on_completed: bool = True
    auto_pause_on_cap: bool = True


PHASES: dict[str, PhaseConfig] = {
    StateGroup.STARTED.value: PhaseConfig(
        state_name="In Progress",
        template_name=KIND_CODING_TASK,  # "coding-task"
        cadence_key="impl",
        fresh_session_on_entry=False,
    ),
    StateGroup.REVIEW.value: PhaseConfig(
        state_name="In Review",
        template_name="review",
        cadence_key="review",
        fresh_session_on_entry=True,
        auto_pause_on_cap=False,
    ),
    StateGroup.TEST.value: PhaseConfig(
        state_name="In Test",
        template_name="test",
        # In Test keeps its own rhythm (12 h); budget is the issue's pool.
        cadence_key="test",
        # The `test` system prompt must land as the actual system prompt
        # of a fresh session, not a user-turn message on a resumed
        # review/implementation conversation. See
        # ``.ai_design/create_test_state/design.md`` §4.3.
        fresh_session_on_entry=True,
        # A cap-exhausted In Test issue stays In Test for a human — see
        # design §4.5.
        auto_pause_on_cap=False,
    ),
}


def is_ticking_state(state) -> bool:
    """Return ``True`` when the given state is the registered ticking
    state for its group.

    Used by the scheduler, the comment continuation handler, the
    bgtasks tick scanner, and the prompt composer to decide whether
    automatic ticking applies.
    """
    if state is None:
        return False
    cfg = PHASES.get(state.group)
    if cfg is None:
        return False
    return state.name == cfg.state_name


def phase_config_for(state) -> Optional[PhaseConfig]:
    """Return the ``PhaseConfig`` for the given state's phase, or
    ``None`` when the state is not a registered ticking state.
    """
    if state is None:
        return None
    cfg = PHASES.get(state.group)
    if cfg is None:
        return None
    if state.name != cfg.state_name:
        return None
    return cfg


def template_name_for(state) -> str:
    """Return the prompt-template name to render for the given state.

    Falls back to the default template name when the state is not in
    the registry.
    """
    cfg = phase_config_for(state)
    if cfg is None:
        return KIND_CODING_TASK
    return cfg.template_name


def cadence_fields_for(state) -> CadenceFields:
    """Return the :class:`CadenceFields` the given state resolves through.

    States that are not a registered ticking state — including a custom
    workspace state inside a ticking group — fall back to the
    implementation pair, matching the pre-phase-split behavior.
    """
    cfg = phase_config_for(state)
    key = cfg.cadence_key if cfg is not None else DEFAULT_CADENCE_KEY
    return CADENCE_FIELDS[key]


def auto_pauses_on_cap(state) -> bool:
    """Return ``True`` when exhausting the budget should auto-Pause the issue.

    Non-ticking states never reach the cap-hit path, so they answer
    ``False``. See :attr:`PhaseConfig.auto_pause_on_cap`.
    """
    cfg = phase_config_for(state)
    if cfg is None:
        return False
    return cfg.auto_pause_on_cap


def cadence_fields_by_group() -> dict[str, CadenceFields]:
    """Return ``state group -> CadenceFields`` for every ticking phase."""
    return {
        group: CADENCE_FIELDS[cfg.cadence_key] for group, cfg in PHASES.items()
    }


def ticking_state_names_by_group() -> dict[str, str]:
    """Return ``state group -> the literal state name that ticks``."""
    return {group: cfg.state_name for group, cfg in PHASES.items()}


__all__ = [
    "CADENCE_FIELDS",
    "DEFAULT_CADENCE_KEY",
    "PHASES",
    "CadenceFields",
    "PhaseConfig",
    "auto_pauses_on_cap",
    "cadence_fields_by_group",
    "cadence_fields_for",
    "is_ticking_state",
    "phase_config_for",
    "template_name_for",
    "ticking_state_names_by_group",
]
