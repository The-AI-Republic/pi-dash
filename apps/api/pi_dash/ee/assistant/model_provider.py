# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""CE model-provider seam for the assistant.

Open source: every user brings their own LLM key (BYOK). The cloud build
overlays this module to additionally offer platform-provided keys to paid
plans (see ``.ai_design/integrate_ai_agent/04-cloud.md`` §3). The assistant
runtime calls :func:`resolve_model_for_user` rather than the BYOK resolver
directly, so the overlay is the single switch point.
"""

from __future__ import annotations

from pi_dash.assistant.runtime.llm import resolve_byok_model


def resolve_model_for_user(user):
    """Return a pydantic-ai model for ``user`` (CE: BYOK only).

    Raises :class:`pi_dash.assistant.errors.AssistantError` with code
    ``llm_config_missing`` / ``assistant_not_configured`` when the user has no
    usable configuration.
    """
    return resolve_byok_model(user)
