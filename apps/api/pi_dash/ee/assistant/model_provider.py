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

from dataclasses import dataclass

from pi_dash.assistant.runtime.llm import get_config, resolve_byok_model
from pi_dash.assistant.runtime.title import generate_byok_title_for_user


@dataclass(frozen=True)
class AgentModelProfile:
    """How the *desktop* agent engine should reach a model for one user.

    Unlike :func:`resolve_model_for_user`, which builds an in-process model for
    server-side callers, this describes a model endpoint the bundled engine can
    call from the user's machine: a base URL plus the lane whose credential the
    desktop will be handed separately. The credential itself is never part of
    this object — see the ``agent-token`` endpoint.

    ``reason_code`` is a :class:`pi_dash.managed_runner.errors.ManagedRunnerReason`
    value and is empty exactly when ``available`` is true.
    """

    available: bool
    lane: str = ""
    base_url: str = ""
    model: str = ""
    reason_code: str = ""


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


def agent_model_profile_for_user(user) -> AgentModelProfile:
    """Describe the model endpoint the desktop engine may call for ``user``.

    CE has exactly one lane, BYOK, and the desktop engine deliberately does not
    serve it: the stored key is decrypted only inside a server process at the
    moment of use and the API has no read path for it, so honouring BYOK on the
    desktop would mean adding the first endpoint that returns a stored key to a
    client. That is a credential-handling decision of its own, not a detail of
    this feature (``.ai_design/managed_runner/design.md`` §12.2, §21).

    The cloud overlay replaces this to return the OpenHub lane, which is what
    makes the managed runner available at all on the hosted build.
    """
    from pi_dash.managed_runner.errors import ManagedRunnerReason

    cfg = get_config(user)
    if cfg and cfg.has_api_key:
        return AgentModelProfile(available=False, lane="byok", reason_code=ManagedRunnerReason.BYOK_UNSUPPORTED)
    return AgentModelProfile(available=False, reason_code=ManagedRunnerReason.LLM_CONFIG_MISSING)


def agent_model_credential_for_user(user) -> tuple[str, object]:
    """Return ``(token, expires_at)`` for the desktop engine's model calls.

    CE has no lane the desktop can use, so there is nothing to hand out; the
    profile already says so and the desktop never reaches this. The cloud
    overlay returns the user's short-lived OpenHub gateway token.

    Raising rather than returning empty keeps the failure loud: a caller that
    ignored :func:`agent_model_profile_for_user` should not silently receive a
    blank credential and produce a 401 deep inside a run.
    """
    from pi_dash.managed_runner.errors import ManagedRunnerReason, ManagedRunnerUnavailable

    raise ManagedRunnerUnavailable(
        ManagedRunnerReason.BYOK_UNSUPPORTED,
        "This build has no model lane the desktop agent can use.",
    )
