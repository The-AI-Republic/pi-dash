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

import hashlib
import threading

from cachetools import TTLCache
from django.conf import settings

from pi_dash.assistant import crypto
from pi_dash.assistant.errors import LLMConfigMissing
from pi_dash.assistant.models import ProviderKind, UserLLMConfig

# Short-lived in-process cache of decrypted BYOK keys, to avoid a KMS Decrypt on
# every assistant turn. Keyed by a hash of the ciphertext so a user changing
# their key auto-invalidates (new ciphertext -> new key -> miss). In-memory per
# worker only: the plaintext never crosses a process/network boundary. Eviction
# is TTL (time) + LRU (capacity) and always safe — a miss just re-decrypts.
# Tunable via ASSISTANT_KEY_CACHE_TTL / _MAXSIZE; TTL=0 disables.
_key_cache: TTLCache | None = None
_key_cache_lock = threading.Lock()


def _get_key_cache() -> TTLCache:
    global _key_cache
    if _key_cache is None:
        ttl = max(1, int(getattr(settings, "ASSISTANT_KEY_CACHE_TTL", 300)))
        maxsize = max(1, int(getattr(settings, "ASSISTANT_KEY_CACHE_MAXSIZE", 1000)))
        _key_cache = TTLCache(maxsize=maxsize, ttl=ttl)
    return _key_cache


def get_decrypted_api_key(cfg: UserLLMConfig) -> str:
    """Decrypt ``cfg``'s BYOK key, served from a short-lived in-process cache.

    Falls back to a direct (uncached) decrypt when caching is disabled
    (``ASSISTANT_KEY_CACHE_TTL <= 0``) or there is no stored key.
    """
    token = cfg.api_key_encrypted
    if int(getattr(settings, "ASSISTANT_KEY_CACHE_TTL", 300) or 0) <= 0 or not token:
        return crypto.decrypt(token)
    cache_key = hashlib.sha256(bytes(token)).hexdigest()
    with _key_cache_lock:
        hit = _get_key_cache().get(cache_key)
    if hit is not None:
        return hit
    # Decrypt OUTSIDE the lock so concurrent decrypts of different keys don't
    # serialize on the KMS round-trip. A rare duplicate decrypt of the same key
    # under contention is harmless (idempotent).
    plaintext = crypto.decrypt(token)
    with _key_cache_lock:
        _get_key_cache()[cache_key] = plaintext
    return plaintext


def get_config(user) -> UserLLMConfig | None:
    return UserLLMConfig.objects.filter(user=user).first()


def resolve_byok_model(user):
    """Return a pydantic-ai model for ``user`` from their stored BYOK config."""
    cfg = get_config(user)
    if cfg is None or not cfg.has_api_key:
        raise LLMConfigMissing("No LLM provider is configured for this user.")
    if not cfg.model_name:
        raise LLMConfigMissing("No model name is configured.")
    # Re-enforce the SSRF guard at execution time (not just at config save) so a
    # base_url that was benign at save time but later re-points (DNS rebinding)
    # at an internal address is still rejected before we connect.
    from pi_dash.assistant import ssrf

    if cfg.base_url and ssrf.is_blocked(cfg.base_url):
        raise LLMConfigMissing("The configured provider endpoint is not allowed.")
    return build_model(
        provider_kind=cfg.provider_kind,
        base_url=cfg.base_url,
        model_name=cfg.model_name,
        api_key=get_decrypted_api_key(cfg),
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
