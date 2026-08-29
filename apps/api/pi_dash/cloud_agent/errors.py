"""Stable error classification and secret-safe diagnostic text."""

from __future__ import annotations

import re

_SECRET_PATTERNS = (
    re.compile(r"(?i)(authorization\s*[:=]\s*(?:bearer\s+)?)[^\s,;]+"),
    re.compile(r"\b(?:sk|gh[opsu])_[A-Za-z0-9_-]{12,}\b"),
    # BYOK provider keys (e.g. OpenAI/Anthropic "sk-..." forms). There is no
    # instance platform key to redact by value — runs use per-user keys.
    re.compile(r"\bsk-[A-Za-z0-9_-]{12,}\b"),
)


def sanitize_error(exc: BaseException) -> str:
    text = str(exc) or type(exc).__name__
    for pattern in _SECRET_PATTERNS:
        text = pattern.sub(lambda match: (match.group(1) if match.lastindex else "") + "[REDACTED]", text)
    return text[:16_000]


def _is_usage_limit(exc: BaseException) -> bool:
    """Classify by exception type — pydantic-ai's UsageLimitExceeded messages
    ("The next request would exceed the request_limit of N") share no stable
    substring worth matching on."""
    try:
        from pydantic_ai.exceptions import UsageLimitExceeded
    except ImportError:
        return False
    return isinstance(exc, UsageLimitExceeded)


def classify_error(exc: BaseException) -> tuple[str, str]:
    text = sanitize_error(exc)
    lowered = text.lower()
    if _is_usage_limit(exc) or "usage limit" in lowered or "would exceed the" in lowered:
        return "iteration_limit", text
    # Deliberately no bare "refused" marker: it matches transport errors like
    # "Connection refused", which must classify as provider_error (FAILED),
    # never as a safety REFUSED terminal state.
    if any(marker in lowered for marker in ("content filter", "content_filter", "safety refusal", "model refused")):
        return "provider_refusal", text
    return "provider_error", text
