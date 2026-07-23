# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""CE model-provider seam for the assistant.

Open source: every user brings their own LLM key (BYOK). The cloud build
overlays this module to additionally offer platform-provided keys to paid
plans (see ``.ai_design/integrate_ai_agent/04-cloud.md`` §3). The assistant
runtime calls :func:`resolve_model_for_user` rather than the BYOK resolver
directly, and lightweight assistant actions call :func:`generate_title_for_user`
instead of BYOK direct helpers, so the overlay is the single switch point.
"""

from __future__ import annotations

from pi_dash.assistant.runtime.llm import get_config, resolve_byok_model
from pi_dash.assistant.runtime.title import generate_byok_title_for_user


def has_usable_llm_config(user) -> bool:
    """True when ``user`` has a usable LLM configuration (CE: a BYOK key).

    Cheap presence check for request-time gating (e.g. rejecting a chat
    message before enqueueing a turn) — must not build a model or decrypt
    anything. The cloud overlay also accepts its platform credentials here.
    """
    cfg = get_config(user)
    return bool(cfg and cfg.has_api_key)


def resolve_model_for_user(user):
    """Return a pydantic-ai model for ``user`` (CE: BYOK only).

    Raises :class:`pi_dash.assistant.errors.AssistantError` with code
    ``llm_config_missing`` / ``assistant_not_configured`` when the user has no
    usable configuration.
    """
    return resolve_byok_model(user)


def generate_title_for_user(user, description: str) -> str:
    """Return a single-prompt generated title for ``user`` (CE: BYOK only)."""
    return generate_byok_title_for_user(user, description)
