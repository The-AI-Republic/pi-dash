# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Canonical token-usage shape for agent runs (PDASHOSS01-188).

Every executor reports usage in its own spelling. This module maps them onto
one bag of counters, stored as ``AgentRun.usage`` / ``RunnerLiveState.usage``::

    {
        "input": 12000,        # every prompt token, cached or not
        "output": 800,         # every completion token, reasoning included
        "total": 12800,        # input + output unless the agent says otherwise
        "cache_read": 9000,    # part of ``input`` served from the prompt cache
        "cache_write": 1200,   # part of ``input`` written to the prompt cache
        "reasoning": 400,      # part of ``output`` spent on reasoning
        "raw": {...},          # the usage object exactly as reported
    }

Only the counters that were actually reported are present. ``raw`` keeps any
counter we have not modelled yet, so it can be promoted to a canonical key
later without a migration and without losing data in the meantime.

The shape is recognised from the keys, not from ``executor_kind``: the
executor kind says *where* a run executed (a local runner, the cloud agent),
not which provider's usage shape it produced, and one runner can drive both
Codex and Claude. The runner's parser (``runner/src/daemon/observability.rs``
``usage_counters``) applies the same mapping — keep the key lists in sync.
"""

from __future__ import annotations

from typing import Any, Dict, Iterable, Mapping, Optional

BIGINT_MAX = 2**63 - 1

CANONICAL_USAGE_KEYS = ("input", "output", "total", "cache_read", "cache_write", "reasoning")

# Provider spellings for each canonical counter, first match wins:
# codex app-server v2 (camelCase), codex legacy / OpenAI Responses /
# Anthropic (snake_case), OpenAI Chat Completions, pydantic-ai ``RunUsage``.
_INPUT_KEYS = ("inputTokens", "input_tokens", "prompt_tokens")
_OUTPUT_KEYS = ("outputTokens", "output_tokens", "completion_tokens")
_TOTAL_KEYS = ("totalTokens", "total_tokens")
_CACHE_READ_KEYS = (
    "cachedInputTokens",
    "cached_input_tokens",
    "cache_read_input_tokens",
    "cache_read_tokens",
)
_CACHE_READ_NESTED = (("prompt_tokens_details", "cached_tokens"), ("input_tokens_details", "cached_tokens"))
_CACHE_WRITE_KEYS = (
    "cacheWriteInputTokens",
    "cache_write_input_tokens",
    "cache_creation_input_tokens",
    "cache_write_tokens",
)
_REASONING_KEYS = ("reasoningOutputTokens", "reasoning_output_tokens")
_REASONING_NESTED = (
    ("completion_tokens_details", "reasoning_tokens"),
    ("output_tokens_details", "reasoning_tokens"),
    ("details", "reasoning_tokens"),
)
# Anthropic reports cache reads / writes *beside* ``input_tokens`` instead of
# inside it. Their presence is what marks an Anthropic usage block.
_ANTHROPIC_CACHE_KEYS = ("cache_read_input_tokens", "cache_creation_input_tokens")


def coerce_token(raw: Any) -> Optional[int]:
    """A non-negative bigint, or ``None`` for anything else."""
    if raw is None or raw == "" or isinstance(raw, bool):
        return None
    try:
        value = int(raw)
    except (TypeError, ValueError):
        return None
    if value < 0 or value > BIGINT_MAX:
        return None
    return value


def _pick(usage: Mapping[str, Any], keys: Iterable[str]) -> Optional[int]:
    for key in keys:
        value = coerce_token(usage.get(key))
        if value is not None:
            return value
    return None


def _pick_nested(usage: Mapping[str, Any], paths: Iterable[tuple[str, str]]) -> Optional[int]:
    for outer, inner in paths:
        container = usage.get(outer)
        if isinstance(container, Mapping):
            value = coerce_token(container.get(inner))
            if value is not None:
                return value
    return None


def _first(*values: Optional[int]) -> Optional[int]:
    return next((value for value in values if value is not None), None)


def _with_total(counters: Dict[str, int]) -> Dict[str, int]:
    if "total" not in counters and "input" in counters and "output" in counters:
        counters["total"] = counters["input"] + counters["output"]
    return counters


def _canonical_counters(usage: Mapping[str, Any]) -> Dict[str, int]:
    counters = {key: coerce_token(usage.get(key)) for key in CANONICAL_USAGE_KEYS}
    return _with_total({key: value for key, value in counters.items() if value is not None})


def _provider_counters(usage: Mapping[str, Any]) -> Dict[str, int]:
    counters: Dict[str, Optional[int]] = {
        "input": _pick(usage, _INPUT_KEYS),
        "output": _pick(usage, _OUTPUT_KEYS),
        "total": _pick(usage, _TOTAL_KEYS),
        "cache_read": _first(_pick(usage, _CACHE_READ_KEYS), _pick_nested(usage, _CACHE_READ_NESTED)),
        "cache_write": _pick(usage, _CACHE_WRITE_KEYS),
        "reasoning": _first(_pick(usage, _REASONING_KEYS), _pick_nested(usage, _REASONING_NESTED)),
    }
    if counters["input"] is not None and any(key in usage for key in _ANTHROPIC_CACHE_KEYS):
        counters["input"] += (counters["cache_read"] or 0) + (counters["cache_write"] or 0)
    return _with_total({key: value for key, value in counters.items() if value is not None})


def normalize_usage(reported: Any) -> Dict[str, Any]:
    """Map a reported usage object onto the canonical shape.

    Accepts either an already-canonical object (the runner's wire ``tokens``,
    which carries the agent's verbatim usage under ``raw``) or a provider
    usage object (Anthropic, OpenAI Chat / Responses, Codex, pydantic-ai),
    which is itself kept verbatim under ``raw``. Returns ``{}`` when nothing
    usable was reported.
    """
    if not isinstance(reported, Mapping) or not reported:
        return {}
    if any(key in reported for key in CANONICAL_USAGE_KEYS):
        usage: Dict[str, Any] = _canonical_counters(reported)
        raw = reported.get("raw")
    else:
        usage = _provider_counters(reported)
        raw = dict(reported)
    if raw not in (None, {}, ""):
        usage["raw"] = raw
    return usage


def merge_usage(*sources: Any) -> Dict[str, Any]:
    """Normalise each source and overlay them, later sources winning per key.

    Used at finalisation to combine the live-state snapshot, the done
    payload's usage and the terminal frame's ``tokens``: a counter the
    fresher source did not report keeps the older source's value, exactly as
    the per-column updates did before usage became one object.
    """
    merged: Dict[str, Any] = {}
    for source in sources:
        merged.update(normalize_usage(source))
    return merged


def flat_token_fields(usage: Any) -> Dict[str, Optional[int]]:
    """The legacy ``input_tokens`` / ``output_tokens`` / ``total_tokens`` view
    of a usage object, for API responses that keep their flat keys."""
    usage = usage if isinstance(usage, Mapping) else {}
    return {
        "input_tokens": coerce_token(usage.get("input")),
        "output_tokens": coerce_token(usage.get("output")),
        "total_tokens": coerce_token(usage.get("total")),
    }
