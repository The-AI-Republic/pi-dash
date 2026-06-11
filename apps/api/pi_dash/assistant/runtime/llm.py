# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""BYOK model resolution: build a pydantic-ai model from the user's config.

Any OpenAI-compatible endpoint (OpenRouter, Together, Fireworks, Groq,
self-hosted vLLM/Ollama, OpenAI) works via ``OpenAIChatModel`` + a custom
``base_url``; Anthropic uses ``AnthropicModel``. See
``.ai_design/integrate_ai_agent/02-backend.md`` §3.
"""

from __future__ import annotations

from pi_dash.assistant import crypto
from pi_dash.assistant.errors import LLMConfigMissing
from pi_dash.assistant.models import ProviderKind, UserLLMConfig


def get_config(user) -> UserLLMConfig | None:
    return UserLLMConfig.objects.filter(user=user).first()


def resolve_byok_model(user):
    """Return a pydantic-ai model for ``user`` from their stored BYOK config."""
    cfg = get_config(user)
    if cfg is None or not cfg.has_api_key:
        raise LLMConfigMissing("No LLM provider is configured for this user.")
    if not cfg.model_name:
        raise LLMConfigMissing("No model name is configured.")
    return build_model(
        provider_kind=cfg.provider_kind,
        base_url=cfg.base_url,
        model_name=cfg.model_name,
        api_key=crypto.decrypt(cfg.api_key_encrypted),
    )


def build_model(*, provider_kind: str, base_url: str, model_name: str, api_key: str):
    """Construct a model object (also used by the test-connection endpoint).

    Imports are local so the rest of the app loads even if pydantic-ai is
    absent (e.g. lightweight management commands).
    """
    if provider_kind == ProviderKind.ANTHROPIC:
        from pydantic_ai.models.anthropic import AnthropicModel
        from pydantic_ai.providers.anthropic import AnthropicProvider

        return AnthropicModel(model_name, provider=AnthropicProvider(api_key=api_key))

    from pydantic_ai.models.openai import OpenAIChatModel
    from pydantic_ai.providers.openai import OpenAIProvider

    return OpenAIChatModel(
        model_name,
        provider=OpenAIProvider(base_url=base_url, api_key=api_key),
    )


def model_label(cfg: UserLLMConfig) -> str:
    return f"{cfg.provider_kind}:{cfg.model_name}"
