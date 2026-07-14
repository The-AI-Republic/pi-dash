# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Single-prompt title generation for assistant-backed work-item creation."""

from __future__ import annotations

import re
from urllib.parse import urlparse

from pi_dash.assistant import ssrf
from pi_dash.assistant.errors import LLMConfigMissing
from pi_dash.assistant.models import ProviderKind, UserLLMConfig
from pi_dash.assistant.runtime.llm import get_config, get_decrypted_api_key

# Work-item titles are capped at 255 chars by the model; keep the AI-generated
# one comfortably inside a single readable line.
_TITLE_MAX_LEN = 255
_TITLE_MAX_OUTPUT_TOKENS = 256
_TITLE_SYSTEM_PROMPT = (
    "You write concise, specific titles for project work items. "
    "Given a work item's description, reply with a single short title "
    "(at most 80 characters) that captures what it is about. "
    "Return only the title text: no surrounding quotes, no trailing "
    "punctuation, no reasoning or analysis, and no preamble such as 'Title:'."
)
_REASONING_BLOCK_RE = re.compile(r"<(think|thinking|reasoning)>.*?</\1>", re.IGNORECASE | re.DOTALL)


def generate_byok_title_for_user(user, description: str) -> str:
    cfg = get_config(user)
    if cfg is None or not cfg.has_api_key:
        raise LLMConfigMissing("No LLM provider is configured for this user.")
    if not cfg.model_name:
        raise LLMConfigMissing("No model name is configured.")
    if cfg.base_url and ssrf.is_blocked(cfg.base_url):
        raise LLMConfigMissing("The configured provider endpoint is not allowed.")

    api_key = get_decrypted_api_key(cfg)
    if cfg.provider_kind == ProviderKind.ANTHROPIC:
        raw_title = _generate_title_anthropic(cfg, api_key, description)
    else:
        raw_title = _generate_title_openai_compatible(cfg, api_key, description)
    return _clean_title(raw_title)


def _generate_title_openai_compatible(cfg: UserLLMConfig, api_key: str, description: str) -> str:
    from openai import OpenAI

    client = OpenAI(api_key=api_key, base_url=cfg.base_url, timeout=20.0)
    completion = client.chat.completions.create(
        model=cfg.model_name,
        messages=[
            {"role": "system", "content": _TITLE_SYSTEM_PROMPT},
            {"role": "user", "content": description},
        ],
        max_tokens=_TITLE_MAX_OUTPUT_TOKENS,
        temperature=0.2,
        **_openai_compatible_extra_options(cfg),
    )
    return _content_to_text(completion.choices[0].message.content)


def _openai_compatible_extra_options(cfg: UserLLMConfig) -> dict:
    if not _uses_deepseek_v4(cfg):
        return {}
    return {"extra_body": {"thinking": {"type": "disabled"}}}


def _uses_deepseek_v4(cfg: UserLLMConfig) -> bool:
    model_name = (cfg.model_name or "").lower()
    if "deepseek-v4" in model_name:
        return True

    hostname = (urlparse(cfg.base_url or "").hostname or "").lower()
    return hostname == "api.deepseek.com" or hostname.endswith(".deepseek.com")


def _generate_title_anthropic(cfg: UserLLMConfig, api_key: str, description: str) -> str:
    from anthropic import Anthropic

    client = Anthropic(api_key=api_key, timeout=20.0)
    message = client.messages.create(
        model=cfg.model_name,
        system=_TITLE_SYSTEM_PROMPT,
        messages=[{"role": "user", "content": description}],
        max_tokens=_TITLE_MAX_OUTPUT_TOKENS,
        temperature=0.2,
    )
    return _content_to_text(message.content)


def _content_to_text(content) -> str:
    if content is None:
        return ""
    if isinstance(content, str):
        return content
    if not isinstance(content, list):
        return str(content)

    parts: list[str] = []
    for block in content:
        block_type = _block_value(block, "type")
        if isinstance(block_type, str) and block_type.lower() in {"reasoning", "thinking"}:
            continue

        text = _block_value(block, "text") or _block_value(block, "content")
        if isinstance(text, str):
            parts.append(text)
        elif isinstance(text, list):
            nested = _content_to_text(text)
            if nested:
                parts.append(nested)
    return "".join(parts)


def _block_value(block, key: str):
    if isinstance(block, dict):
        return block.get(key)
    return getattr(block, key, None)


def _clean_title(raw: str) -> str:
    """Normalize the model's reply into a single-line title within the length cap."""
    title = (raw or "").strip()
    if not title:
        return ""
    title = _strip_reasoning_blocks(title)
    if not title:
        return ""
    # The model occasionally wraps the title in quotes or spreads it over lines.
    title = title.splitlines()[0].strip().strip("\"'").strip()
    if title.lower().startswith("title:"):
        title = title.split(":", 1)[1].strip()
    title = title.rstrip(".,:;!?").strip()
    if len(title) > _TITLE_MAX_LEN:
        title = title[:_TITLE_MAX_LEN].rstrip()
    return title


def _strip_reasoning_blocks(text: str) -> str:
    previous = None
    while previous != text:
        previous = text
        text = _REASONING_BLOCK_RE.sub("", text).strip()
    return text
