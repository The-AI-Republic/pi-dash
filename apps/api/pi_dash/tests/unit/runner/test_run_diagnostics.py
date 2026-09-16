# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from types import SimpleNamespace

import pytest

from pi_dash.runner.diagnostics import classify_run_error, enrich_run_error, infer_agent_label


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


@pytest.mark.parametrize(
    ("runner_name", "expected"),
    [
        ("workx_codex01", "Codex"),
        ("workx-claude-code-01", "Claude Code"),
        ("workx-cursor-agent-01", "Cursor"),
        ("workx-open-claw-01", "OpenClaw"),
        ("workx-grok-01", "Grok"),
        ("workx-muse-code-01", "Muse Code"),
    ],
)
def test_infer_agent_label_covers_every_backend_from_runner_name(runner_name, expected):
    """Every backend is recognised from the runner name.

    The name is asserted rather than ``capabilities`` to keep this a
    name-only signal test; capabilities has its own coverage in
    ``test_populated_capability_identifies_the_agent_over_neutral_signals``
    below.
    """
    runner = SimpleNamespace(name=runner_name, host_label="", capabilities=[], dev_machine=None)

    assert infer_agent_label(runner=runner) == expected


@pytest.mark.parametrize(
    ("capability", "expected"),
    [
        ("agent:codex", "Codex"),
        ("agent:claude_code", "Claude Code"),
        ("agent:cursor_agent", "Cursor"),
        ("agent:open_claw", "OpenClaw"),
        ("agent:grok", "Grok"),
        ("agent:muse_code", "Muse Code"),
    ],
)
def test_infer_agent_label_reads_agent_capability_when_present(capability, expected):
    """Forward-compatible: the matcher handles the serde snake_case spelling.

    ``open_claw`` and ``muse_code`` are the spellings an ``agent:<kind>``
    capability would carry, and neither was matched before. This path is now
    exercised in production — ``apply_hello`` writes the capability; see
    ``test_populated_capability_identifies_the_agent_over_neutral_signals``.
    """
    runner = SimpleNamespace(name="", host_label="", capabilities=[capability], dev_machine=None)

    assert infer_agent_label(runner=runner) == expected


def test_populated_capability_identifies_the_agent_over_neutral_signals():
    """The daemon now reports its ``AgentKind``, so capabilities is a writer.

    ``apply_hello`` persists the reported kind as an ``agent:<kind>``
    capability (see ``session_service._agent_capabilities`` and its
    ``test_apply_hello_persists_agent_kind_capability`` coverage). It stays
    listed in the serializer's ``read_only_fields`` — an API *client* still
    cannot set it; the value is written server-side from the Hello payload.

    This closes the gap the old ``test_capabilities_is_not_yet_populated_in_production``
    pinned: a Grok / Muse Code runner left on its agent's default model, with a
    neutral host name and no model slug, previously fell through to the generic
    label. With the capability populated it is now identified exactly.
    """
    from pi_dash.runner.serializers import RunnerSerializer

    assert "capabilities" in RunnerSerializer.Meta.read_only_fields

    runner = SimpleNamespace(
        name="mini-build",
        host_label="host-01",
        capabilities=["agent:muse_code"],
        dev_machine=None,
    )

    assert infer_agent_label(runner=runner) == "Muse Code"


@pytest.mark.parametrize(
    "host_label",
    ["museum-pi", "amused-badger", "grokking-notes", "provoked-box"],
)
def test_infer_agent_label_ignores_incidental_substrings(host_label):
    """A host name that merely contains "muse"/"grok" is not that agent.

    Generated host names really do produce words like "amused", so the bare
    forms match on a word boundary rather than as substrings.
    """
    runner = SimpleNamespace(name="", host_label=host_label, capabilities=[], dev_machine=None)

    assert infer_agent_label(runner=runner) == ""


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
