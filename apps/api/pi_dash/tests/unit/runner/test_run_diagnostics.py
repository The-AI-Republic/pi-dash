# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from types import SimpleNamespace

import pytest

from pi_dash.runner.diagnostics import classify_run_error, enrich_run_error


pytestmark = pytest.mark.unit


def test_classify_agent_authentication_error():
    diagnostic = classify_run_error("Failed to authenticate. API Error: 401 Invalid authentication credentials")

    assert diagnostic is not None
    assert diagnostic["source"] == "agent"
    assert diagnostic["kind"] == "agent_authentication"
    assert diagnostic["summary"] == "Failed to authenticate. API Error: 401 Invalid authentication credentials"


def test_enrich_non_401_auth_error_does_not_claim_401():
    """A 403 (or code-less) auth failure must not be relabeled as a 401."""
    raw = "Failed to authenticate. API Error: 403 Forbidden"

    enriched = enrich_run_error(raw)

    assert enriched.startswith("403 authentication_failed\n")
    assert "401" not in enriched.splitlines()[0]


def test_enrich_codeless_auth_error_uses_neutral_header():
    raw = "Failed to authenticate: token refresh returned no credentials"

    enriched = enrich_run_error(raw)

    assert enriched.startswith("authentication_failed\n")


def test_infer_agent_label_prefers_runner_over_error_mention():
    """An unrelated 'claude' in the error text must not mislabel a Codex run."""
    runner = SimpleNamespace(
        name="workx_codex01",
        host_label="mini-build",
        capabilities=["agent:codex"],
        dev_machine=None,
    )
    # Error references a path that merely contains "claude".
    raw = "Failed to authenticate. Check /Users/claude/.codex/auth.json"

    enriched = enrich_run_error(raw, runner=runner)

    assert "re-authenticate Codex" in enriched
    diagnostic = classify_run_error(enriched)
    assert diagnostic is not None
    assert diagnostic["source_label"] == "Codex"


def test_classify_agent_model_access_error():
    diagnostic = classify_run_error("Selected model 'claude-fable-5' may not exist or you may not have access to it.")

    assert diagnostic is not None
    assert diagnostic["source"] == "agent"
    assert diagnostic["kind"] == "agent_model_access"


def test_classify_pidash_cloud_runner_registration_error():
    diagnostic = classify_run_error('{"detail":"runner_not_found"}')

    assert diagnostic is not None
    assert diagnostic["source"] == "pidash_cloud"
    assert diagnostic["kind"] == "runner_registration"


def test_empty_error_has_no_diagnostic():
    assert classify_run_error("") is None


def test_enrich_agent_authentication_error_adds_actionable_cloud_log_message():
    runner = SimpleNamespace(
        name="workx_claude01",
        host_label="mini-build",
        capabilities=["agent:claude_code"],
        dev_machine=SimpleNamespace(label="Mac Mini", host_label="mac-mini.local"),
    )
    raw = "Failed to authenticate. API Error: 401 Invalid authentication credentials"

    enriched = enrich_run_error(raw, runner=runner)

    assert enriched.startswith("401 authentication_failed\n")
    assert (
        'AI agent: Claude Code auth appears expired or invalid. Go to the dev machine "Mac Mini" '
        'for runner "workx_claude01" and re-authenticate Claude Code'
    ) in enriched
    assert "Raw agent error:\nFailed to authenticate. API Error: 401 Invalid authentication credentials" in enriched

    diagnostic = classify_run_error(enriched)
    assert diagnostic is not None
    assert diagnostic["source"] == "agent"
    assert diagnostic["source_label"] == "Claude Code"
    assert diagnostic["summary"] == "401 authentication_failed"
