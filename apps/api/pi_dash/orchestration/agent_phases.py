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

Cadence (interval / max_ticks) is intentionally **not** on ``PhaseConfig``.
It stays centrally managed on ``Project`` (with per-issue overrides on
``IssueAgentTicker``) so an operator can retune review rhythm without a
code deploy. PR B introduces the per-phase split on ``Project``;
``PhaseConfig`` itself stays narrow forever.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

from pi_dash.db.models.state import StateGroup
from pi_dash.prompting.models import PromptTemplate


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
    """

    state_name: str
    template_name: str
    fresh_session_on_entry: bool
    disarm_on_completed: bool = True


PHASES: dict[str, PhaseConfig] = {
    StateGroup.STARTED.value: PhaseConfig(
        state_name="In Progress",
        template_name=PromptTemplate.DEFAULT_NAME,  # "coding-task"
        fresh_session_on_entry=False,
    ),
    # PR B adds the StateGroup.REVIEW entry for the In Review phase.
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
        return PromptTemplate.DEFAULT_NAME
    return cfg.template_name


__all__ = [
    "PHASES",
    "PhaseConfig",
    "is_ticking_state",
    "phase_config_for",
    "template_name_for",
]
