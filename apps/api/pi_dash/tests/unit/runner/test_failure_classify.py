# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Table-driven coverage for the canonical failure taxonomy (PDASHOSS01-183).

One row per reason with real captured error strings per agent, plus the
digit-boundary false positives the classifier must not trip on: a bare
substring match on ``401`` / ``402`` / ``429`` / ``529`` / ``403`` would
fire on ``"402913 tokens"``, ``"15290ms"`` or ``"exit status 4030"`` and
file a process crash under a provider bucket.
"""

from __future__ import annotations

import pytest

from pi_dash.runner.failure import (
    FailurePolicy,
    RunFailureReason,
    classify,
    is_agent_side,
    policy_for,
    repeated_failure_limit_for,
)

pytestmark = pytest.mark.unit


# ---------------------------------------------------------------------------
# Agent-side text rules — real captured strings per agent
# ---------------------------------------------------------------------------

AGENT_TEXT_CASES = [
    # provider_auth_or_access
    ("Invalid API key · Please run /login", RunFailureReason.AGENT_PROVIDER_AUTH),  # Claude Code
    ("stream error: 401 Unauthorized: token expired", RunFailureReason.AGENT_PROVIDER_AUTH),  # Codex
    ("Error: unauthorized (401) — check your Cursor session", RunFailureReason.AGENT_PROVIDER_AUTH),
    ("failed to authenticate with the model gateway", RunFailureReason.AGENT_PROVIDER_AUTH),
    (
        "OAuth token has expired. Please obtain a new token or refresh your existing token.",
        RunFailureReason.AGENT_PROVIDER_AUTH,
    ),
    ("403 Forbidden", RunFailureReason.AGENT_PROVIDER_AUTH),
    # provider_quota_limit
    ("Your credit balance is too low to access the Anthropic API.", RunFailureReason.AGENT_PROVIDER_QUOTA),
    ("402 Payment Required", RunFailureReason.AGENT_PROVIDER_QUOTA),
    ("You have hit your usage limit.", RunFailureReason.AGENT_PROVIDER_QUOTA),
    (
        # Claude Code's out-of-credits message, verbatim from a captured run
        # (mentions "model" and "credits" but neither a status code nor
        # "usage limit", so it needs its own alternative in the pattern).
        "You're out of usage credits. Switch to another model, or manage usage "
        "credits at claude.ai/settings/usage?from=cc_cli_limit_message, to continue.",
        RunFailureReason.AGENT_PROVIDER_QUOTA,
    ),
    ("5-hour limit reached ∙ resets 3am", RunFailureReason.AGENT_PROVIDER_QUOTA),
    # provider_capacity_or_rate_limit
    ("429 Too Many Requests", RunFailureReason.AGENT_PROVIDER_CAPACITY),
    ("overloaded_error: Overloaded", RunFailureReason.AGENT_PROVIDER_CAPACITY),
    ("API error 529: upstream capacity constraint", RunFailureReason.AGENT_PROVIDER_CAPACITY),
    (
        "rate_limit_error: Number of request tokens has exceeded your per-minute rate limit",
        RunFailureReason.AGENT_PROVIDER_CAPACITY,
    ),
    # a 403 used for a concurrency rejection is capacity, not auth
    ("403: Number of concurrent connections has exceeded your plan's limit", RunFailureReason.AGENT_PROVIDER_CAPACITY),
    # provider_server_error
    ("500 Internal Server Error", RunFailureReason.AGENT_PROVIDER_SERVER_ERROR),
    ("api_error: Internal server error", RunFailureReason.AGENT_PROVIDER_SERVER_ERROR),
    ("502 Bad Gateway from api.openai.com", RunFailureReason.AGENT_PROVIDER_SERVER_ERROR),
    # provider_network
    ("connection reset by peer", RunFailureReason.AGENT_PROVIDER_NETWORK),
    ("getaddrinfo ENOTFOUND api.anthropic.com", RunFailureReason.AGENT_PROVIDER_NETWORK),
    ("fetch failed: socket hang up", RunFailureReason.AGENT_PROVIDER_NETWORK),
    ("Could not resolve host: api.x.ai", RunFailureReason.AGENT_PROVIDER_NETWORK),
    # context_overflow
    ("prompt is too long: 213462 tokens > 200000 maximum", RunFailureReason.AGENT_CONTEXT_OVERFLOW),
    ("This model's maximum context length is 272000 tokens.", RunFailureReason.AGENT_CONTEXT_OVERFLOW),
    ("context_length_exceeded", RunFailureReason.AGENT_CONTEXT_OVERFLOW),
    # model_not_found_or_unavailable
    ("The selected model may not exist or you may not have access to it.", RunFailureReason.AGENT_MODEL_NOT_FOUND),
    ("model_not_found: gpt-5.1-codex-mini", RunFailureReason.AGENT_MODEL_NOT_FOUND),
    ("Unknown model 'grok-5-fast'", RunFailureReason.AGENT_MODEL_NOT_FOUND),
    # missing_executable
    ("zsh: command not found: codex", RunFailureReason.AGENT_MISSING_EXECUTABLE),
    (
        "failed to spawn agent process: No such file or directory (os error 2)",
        RunFailureReason.AGENT_MISSING_EXECUTABLE,
    ),
    ("'claude' is not recognized as an internal or external command", RunFailureReason.AGENT_MISSING_EXECUTABLE),
    # missing_config
    ("Error: No API key configured. Set ANTHROPIC_API_KEY or run /login.", RunFailureReason.AGENT_MISSING_CONFIG),
    ("config file not found: ~/.codex/config.toml", RunFailureReason.AGENT_MISSING_CONFIG),
    # unsupported_version
    (
        "This version of the CLI is no longer supported. Please upgrade to continue.",
        RunFailureReason.AGENT_UNSUPPORTED_VERSION,
    ),
    (
        "protocol version mismatch: daemon speaks v4, agent bridge requires v3",
        RunFailureReason.AGENT_UNSUPPORTED_VERSION,
    ),
    # empty_or_unparseable_output
    ("done-signal parse error: missing pi-dash-done fence", RunFailureReason.AGENT_EMPTY_OUTPUT),
    ("agent produced no output before exiting", RunFailureReason.AGENT_EMPTY_OUTPUT),
    # process_failure
    ("codex exited with status 101", RunFailureReason.AGENT_PROCESS_FAILURE),
    ("agent process terminated by signal SIGKILL", RunFailureReason.AGENT_PROCESS_FAILURE),
    ("thread 'main' panicked at src/agent.rs:42", RunFailureReason.AGENT_PROCESS_FAILURE),
    # unknown — fail-safe bucket
    ("something inexplicable happened", RunFailureReason.AGENT_UNKNOWN),
    ("", RunFailureReason.AGENT_UNKNOWN),
]


@pytest.mark.parametrize("text,expected", AGENT_TEXT_CASES, ids=[c[0][:48] or "(empty)" for c in AGENT_TEXT_CASES])
def test_agent_text_classification(text, expected):
    assert classify(text, runner_reason="agent_crash") == expected


# Digit-boundary false positives: numbers that merely *contain* an HTTP
# status code must never select a provider bucket.
FALSE_POSITIVE_CASES = [
    # 4030 contains 403 — process failure, not auth
    ("agent exited with status 4030", RunFailureReason.AGENT_PROCESS_FAILURE),
    # 15290 contains 529 — process failure, not capacity
    ("agent crashed after 15290ms", RunFailureReason.AGENT_PROCESS_FAILURE),
    # 402913 contains 402 and 401 — nothing provider-ish in this crash
    ("consumed 402913 tokens then crashed", RunFailureReason.AGENT_PROCESS_FAILURE),
    # ...but the same token count inside an overflow message is an overflow
    ("prompt is too long: 402913 tokens > 200000 maximum", RunFailureReason.AGENT_CONTEXT_OVERFLOW),
    # 5000 contains 500 — not a server error
    ("retried for 5000 ms, giving up; exit code 1", RunFailureReason.AGENT_PROCESS_FAILURE),
]


@pytest.mark.parametrize("text,expected", FALSE_POSITIVE_CASES, ids=[c[0][:48] for c in FALSE_POSITIVE_CASES])
def test_digit_boundary_false_positives(text, expected):
    assert classify(text, runner_reason="agent_crash") == expected


# ---------------------------------------------------------------------------
# Platform side — the runner's structured reason wins by construction
# ---------------------------------------------------------------------------

RUNNER_REASON_CASES = [
    ("workspace_setup", RunFailureReason.WORKSPACE_SETUP),
    ("git_auth", RunFailureReason.GIT_AUTH),
    ("network", RunFailureReason.NETWORK),
    ("timeout", RunFailureReason.TIMEOUT),
    ("max_turns", RunFailureReason.MAX_TURNS),
    ("internal", RunFailureReason.INTERNAL),
    ("daemon_restart", RunFailureReason.DAEMON_RESTART),
    ("assign_rejected_busy", RunFailureReason.ASSIGN_REJECTED_BUSY),
    ("resume_unavailable", RunFailureReason.INTERNAL),
]


@pytest.mark.parametrize("runner_reason,expected", RUNNER_REASON_CASES)
def test_old_runner_reasons_map_platform_side(runner_reason, expected):
    """Every value an old runner can send still classifies — the wire stays
    backward compatible."""
    assert classify("anything at all", runner_reason=runner_reason) == expected


def test_pre_launch_failure_never_reaches_agent_rules():
    """A workspace-setup failure whose OS error text *looks* like an agent
    rule (401 in a path, 'command not found') stays platform-side."""
    text = "clone failed: /work/401-branch: command not found"
    assert classify(text, runner_reason="workspace_setup") == RunFailureReason.WORKSPACE_SETUP


def test_codex_crash_routes_to_agent_rules():
    assert (
        classify("401 Unauthorized", runner_reason="codex_crash")
        == RunFailureReason.AGENT_PROVIDER_AUTH
    )


# Cloud-side writers carry an error_code instead of a runner reason.
ERROR_CODE_CASES = [
    ("run_timeout", RunFailureReason.TIMEOUT),
    ("dispatch_timeout", RunFailureReason.TIMEOUT),
    ("heartbeat_reaped", RunFailureReason.RUNNER_OFFLINE),
    ("desktop_not_connected", RunFailureReason.RUNNER_OFFLINE),
    ("llm_config_missing", RunFailureReason.AGENT_MISSING_CONFIG),
    ("prompt_too_large", RunFailureReason.AGENT_CONTEXT_OVERFLOW),
]


@pytest.mark.parametrize("code,expected", ERROR_CODE_CASES)
def test_error_code_hints(code, expected):
    assert classify("detail text", error_code=code) == expected


# Cloud-written detail strings with neither reason nor code.
INFRA_TEXT_CASES = [
    ("daemon shutdown requested (SIGTERM at 12:00:01)", RunFailureReason.DAEMON_RESTART),
    ("agent stalled: no events for >900s", RunFailureReason.TIMEOUT),
    (
        "reaped by heartbeat: runner reported in_flight_run=(none) but cloud had this run marked busy",
        RunFailureReason.RUNNER_OFFLINE,
    ),
]


@pytest.mark.parametrize("text,expected", INFRA_TEXT_CASES)
def test_infra_text_prefixes(text, expected):
    assert classify(text) == expected


# ---------------------------------------------------------------------------
# Policy — the fail-safe contract
# ---------------------------------------------------------------------------


def test_policy_groups():
    assert policy_for(RunFailureReason.AGENT_PROVIDER_NETWORK) == FailurePolicy.RETRY_FREE
    assert policy_for(RunFailureReason.DAEMON_RESTART) == FailurePolicy.RETRY_FREE
    assert policy_for(RunFailureReason.RUNNER_OFFLINE) == FailurePolicy.RETRY_FREE
    assert policy_for(RunFailureReason.ASSIGN_REJECTED_BUSY) == FailurePolicy.RETRY_FREE
    assert policy_for(RunFailureReason.AGENT_PROVIDER_AUTH) == FailurePolicy.NEEDS_HUMAN
    assert policy_for(RunFailureReason.AGENT_PROVIDER_QUOTA) == FailurePolicy.NEEDS_HUMAN
    assert policy_for(RunFailureReason.GIT_AUTH) == FailurePolicy.NEEDS_HUMAN
    assert policy_for(RunFailureReason.WORKSPACE_SETUP) == FailurePolicy.NEEDS_HUMAN
    assert policy_for(RunFailureReason.AGENT_MISSING_EXECUTABLE) == FailurePolicy.NEEDS_HUMAN
    assert policy_for(RunFailureReason.AGENT_CONTEXT_OVERFLOW) == FailurePolicy.FRESH_SESSION
    assert policy_for(RunFailureReason.TIMEOUT) == FailurePolicy.RETRY_NORMAL
    assert policy_for(RunFailureReason.AGENT_PROVIDER_SERVER_ERROR) == FailurePolicy.RETRY_NORMAL
    assert policy_for(RunFailureReason.AGENT_UNKNOWN) == FailurePolicy.RETRY_NORMAL


def test_unknown_reason_fails_safe_onto_the_normal_clock():
    """Misclassification must never land in the free-retry bucket."""
    assert policy_for("agent_error.some_future_value") == FailurePolicy.RETRY_NORMAL
    assert policy_for("") == FailurePolicy.RETRY_NORMAL
    assert policy_for("garbage") == FailurePolicy.RETRY_NORMAL


def test_free_retry_reasons_get_their_allowance_before_the_backstop():
    from pi_dash.runner.failure import FREE_RETRY_MAX_ATTEMPTS, REPEATED_FAILURE_LIMIT

    assert repeated_failure_limit_for(RunFailureReason.AGENT_PROVIDER_NETWORK) == max(
        REPEATED_FAILURE_LIMIT, FREE_RETRY_MAX_ATTEMPTS + 1
    )
    assert repeated_failure_limit_for(RunFailureReason.AGENT_PROVIDER_AUTH) == REPEATED_FAILURE_LIMIT


def test_agent_side_prefix():
    assert is_agent_side("agent_error.unknown")
    assert not is_agent_side("timeout")
    for value in RunFailureReason.values:
        assert is_agent_side(value) == value.startswith("agent_error.")


def test_runner_stall_detail_with_stderr_tail_stays_platform_side():
    """The runner stall watchdog's detail embeds the last command's stderr
    tail; an npm ENOTFOUND inside it is not a provider network failure."""
    detail = (
        "no agent frames for 5 minutes; last command: `npm install`; "
        "stderr tail (1 line(s)):\n  npm err! ENOTFOUND registry.npmjs.org"
    )
    assert classify(detail) == RunFailureReason.TIMEOUT


def test_internal_reason_consults_text_rules_for_codex_bridge_errors():
    """The Codex bridge reports agent-side turn/API errors as ``internal``;
    the server text rules recover the real bucket from the detail, and a
    detail that says nothing agent-ish stays platform-internal."""
    assert (
        classify("stream error: 401 Unauthorized", runner_reason="internal")
        == RunFailureReason.AGENT_PROVIDER_AUTH
    )
    assert (
        classify(
            "The selected model may not exist or you may not have access to it.",
            runner_reason="internal",
        )
        == RunFailureReason.AGENT_MODEL_NOT_FOUND
    )
    assert (
        classify("turn/completed without conclusion", runner_reason="internal")
        == RunFailureReason.INTERNAL
    )


def test_cancelled_runner_reason_maps_platform_side():
    assert classify("openclaw stopReason: cancelled", runner_reason="cancelled") == RunFailureReason.INTERNAL
