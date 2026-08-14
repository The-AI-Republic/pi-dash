"""Stable error classification and secret-safe diagnostic text."""

from __future__ import annotations

import re

from django.conf import settings

_SECRET_PATTERNS = (
    re.compile(r"(?i)(authorization\s*[:=]\s*(?:bearer\s+)?)[^\s,;]+"),
    re.compile(r"\b(?:sk|gh[opsu])_[A-Za-z0-9_-]{12,}\b"),
)


def sanitize_error(exc: BaseException) -> str:
    text = str(exc) or type(exc).__name__
    configured_key = getattr(settings, "CLOUD_AGENT_MODEL_API_KEY", "")
    if configured_key:
        text = text.replace(configured_key, "[REDACTED]")
    for pattern in _SECRET_PATTERNS:
        text = pattern.sub(lambda match: (match.group(1) if match.lastindex else "") + "[REDACTED]", text)
    return text[:16_000]


def classify_error(exc: BaseException) -> tuple[str, str]:
    text = sanitize_error(exc)
    lowered = text.lower()
    if "usage limit" in lowered or "tool call limit" in lowered:
        return "iteration_limit", text
    if any(marker in lowered for marker in ("content filter", "content_filter", "safety refusal", "refused")):
        return "provider_refusal", text
    return "provider_error", text
