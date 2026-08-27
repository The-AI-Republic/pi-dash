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

Cadence *values* (interval / max_ticks) are intentionally **not** on
``PhaseConfig``. They stay centrally managed on ``Project`` (with
per-issue overrides on ``IssueAgentTicker``) so an operator can retune a
phase's rhythm without a code deploy. What ``PhaseConfig`` carries is the
``cadence_key`` — which *column pair* a phase resolves through, via
``CADENCE_FIELDS``. Each ticking phase owns an independent pair, so
phases never read or write each other's budget.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

from pi_dash.db.models.state import StateGroup
from pi_dash.prompting.recipes import KIND_CODING_TASK


@dataclass(frozen=True)
class CadenceFields:
    """Which ticker/project columns hold one phase's cadence.

    Every ticking phase owns an independent field pair — a per-issue
    override on ``IssueAgentTicker`` and a project-level default on
    ``Project``. Phases are siblings: a phase reads and writes only its
    own pair, so a cap grant or interval tweak made in one phase can
    never leak into another. Resolving through this table (rather than
    an ``if group == ...`` chain at each call site) means adding or
    retuning a phase is a change to this one dict.
    """

    ticker_interval: str
    ticker_max_ticks: str
    project_interval: str
    project_max_ticks: str
    default_interval: int
    default_max_ticks: int


#: Cadence key → the columns that phase resolves through. Keys are
#: opaque labels referenced by ``PhaseConfig.cadence_key``; they are
#: deliberately *not* state-group values, so a future phase can share a
#: pair (or a group can be renamed) without touching the schema.
CADENCE_FIELDS: dict[str, CadenceFields] = {
    "impl": CadenceFields(
        ticker_interval="interval_seconds",
        ticker_max_ticks="max_ticks",
        project_interval="agent_default_interval_seconds",
        project_max_ticks="agent_default_max_ticks",
        default_interval=43200,  # 12 h
        default_max_ticks=24,  # 3 days
    ),
    "review": CadenceFields(
        ticker_interval="review_interval_seconds",
        ticker_max_ticks="review_max_ticks",
        project_interval="agent_review_default_interval_seconds",
        project_max_ticks="agent_review_default_max_ticks",
        default_interval=28800,  # 8 h
        default_max_ticks=4,  # 32 h window
    ),
    "test": CadenceFields(
        ticker_interval="test_interval_seconds",
        ticker_max_ticks="test_max_ticks",
        project_interval="agent_test_default_interval_seconds",
        project_max_ticks="agent_test_default_max_ticks",
        default_interval=43200,  # 12 h
        default_max_ticks=3,  # 36 h window
    ),
}

#: Phases that are not registered ticking states (or states outside the
#: registry entirely) fall back to the implementation pair — the
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
            Key into :data:`CADENCE_FIELDS` naming the override/default
            column pair this phase's interval and cap resolve through.
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
        # In Test is a sibling of In Review, not a variant of it: its own
        # cadence pair, its own budget. The two never share a column.
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
    """Return ``state group -> CadenceFields`` for every ticking phase.

    For callers that must branch on the group in SQL (the due-ticker
    scan annotates the effective cap in the database) and therefore
    cannot go through :func:`cadence_fields_for`, which needs a loaded
    ``State``.
    """
    return {
        group: CADENCE_FIELDS[cfg.cadence_key] for group, cfg in PHASES.items()
    }


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
]
