# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-provider mapping of reported token usage onto the canonical shape
(PDASHOSS01-188)."""

from __future__ import annotations

import pytest

from pi_dash.runner.services.usage import flat_token_fields, merge_usage, normalize_usage


@pytest.mark.unit
def test_anthropic_usage_folds_cache_into_input():
    # Verbatim Claude Code ``result.usage`` shape.
    reported = {
        "cache_creation": {"ephemeral_1h_input_tokens": 15222, "ephemeral_5m_input_tokens": 0},
        "cache_creation_input_tokens": 15222,
        "cache_read_input_tokens": 15553,
        "inference_geo": "not_available",
        "input_tokens": 2,
        "output_tokens": 8,
        "service_tier": "standard",
    }
    usage = normalize_usage(reported)
    assert usage["input"] == 2 + 15222 + 15553
    assert usage["output"] == 8
    assert usage["total"] == 2 + 15222 + 15553 + 8
    assert usage["cache_read"] == 15553
    assert usage["cache_write"] == 15222
    assert "reasoning" not in usage
    # Counters we don't model (the 1h / 5m cache split) survive under raw.
    assert usage["raw"] == reported


@pytest.mark.unit
def test_openai_chat_completions_usage_reads_nested_details():
    reported = {
        "prompt_tokens": 1000,
        "completion_tokens": 300,
        "total_tokens": 1300,
        "prompt_tokens_details": {"cached_tokens": 600, "audio_tokens": 0},
        "completion_tokens_details": {"reasoning_tokens": 120, "accepted_prediction_tokens": 0},
    }
    usage = normalize_usage(reported)
    assert {k: usage[k] for k in ("input", "output", "total", "cache_read", "reasoning")} == {
        "input": 1000,
        "output": 300,
        "total": 1300,
        "cache_read": 600,
        "reasoning": 120,
    }
    # OpenAI's cached tokens are already inside prompt_tokens — no fold.
    assert "cache_write" not in usage
    assert usage["raw"] == reported


@pytest.mark.unit
def test_openai_responses_usage_reads_nested_details():
    reported = {
        "input_tokens": 500,
        "input_tokens_details": {"cached_tokens": 0},
        "output_tokens": 90,
        "output_tokens_details": {"reasoning_tokens": 64},
        "total_tokens": 590,
    }
    usage = normalize_usage(reported)
    # A zero counter is a real report, not a missing one.
    assert usage["cache_read"] == 0
    assert usage["reasoning"] == 64
    assert usage["total"] == 590


@pytest.mark.unit
def test_codex_app_server_usage_is_camel_case():
    reported = {
        "cacheWriteInputTokens": 0,
        "cachedInputTokens": 11008,
        "inputTokens": 21763,
        "outputTokens": 551,
        "reasoningOutputTokens": 133,
        "totalTokens": 22314,
    }
    usage = normalize_usage(reported)
    assert usage["input"] == 21763
    assert usage["cache_read"] == 11008
    assert usage["cache_write"] == 0
    assert usage["reasoning"] == 133
    assert usage["total"] == 22314


@pytest.mark.unit
def test_pydantic_ai_run_usage_shape():
    reported = {
        "input_tokens": 400,
        "cache_read_tokens": 300,
        "cache_write_tokens": 20,
        "output_tokens": 50,
        "details": {"reasoning_tokens": 12},
        "requests": 2,
        "tool_calls": 1,
        "total_tokens": 450,
    }
    usage = normalize_usage(reported)
    # pydantic-ai already counts cache tokens inside input_tokens.
    assert usage["input"] == 400
    assert usage["cache_read"] == 300
    assert usage["cache_write"] == 20
    assert usage["reasoning"] == 12
    assert usage["raw"]["requests"] == 2


@pytest.mark.unit
def test_runner_wire_tokens_are_canonical_and_keep_raw():
    reported = {
        "input": 100,
        "output": 20,
        "total": 120,
        "cache_read": 60,
        "raw": {"total": {"inputTokens": 100, "brandNewCounter": 7}},
    }
    usage = normalize_usage(reported)
    assert usage == reported


@pytest.mark.unit
def test_missing_total_is_input_plus_output():
    assert normalize_usage({"input_tokens": 10, "output_tokens": 20})["total"] == 30
    assert normalize_usage({"input": 10, "output": 20})["total"] == 30
    # Only one side reported: no invented total.
    assert "total" not in normalize_usage({"output": 20})


@pytest.mark.unit
def test_unknown_shape_is_kept_under_raw():
    usage = normalize_usage({"weird_counter": 5, "another": {"x": 1}})
    assert usage == {"raw": {"weird_counter": 5, "another": {"x": 1}}}


@pytest.mark.unit
@pytest.mark.parametrize("reported", [None, {}, [], "12", 5])
def test_nothing_reported_normalises_to_empty(reported):
    assert normalize_usage(reported) == {}


@pytest.mark.unit
def test_out_of_range_and_bogus_counters_are_dropped():
    usage = normalize_usage({"input": 2**63, "output": -1, "total": True, "cache_read": "12"})
    assert usage == {"cache_read": 12}


@pytest.mark.unit
def test_merge_later_source_wins_per_counter():
    merged = merge_usage(
        {"input": 10, "output": 20, "total": 30, "cache_read": 5},
        None,
        {"input": 11, "output": 21, "total": 32},
    )
    assert merged == {"input": 11, "output": 21, "total": 32, "cache_read": 5}


@pytest.mark.unit
def test_flat_token_fields():
    assert flat_token_fields({"input": 1, "output": 2, "total": 3, "cache_read": 1}) == {
        "input_tokens": 1,
        "output_tokens": 2,
        "total_tokens": 3,
    }
    assert flat_token_fields(None) == {"input_tokens": None, "output_tokens": None, "total_tokens": None}
