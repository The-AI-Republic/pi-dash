# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared pin for project bodies whose stored ``settings`` is ``{}``.

Django echoes the stored blob verbatim, so a project with no settings
keys reads ``{}``. Rust renders the validated settings with every
registered key's default filled in (DIFF-51 in
.ai_design/validation/differences.md, NEWPIDASH-278): the
``issue_run_policy`` key (DIFF-53) and the lean-workflow
``agent_stages`` key (#3403). The values mirror the defaults in
``rust-api/crates/services/src/domain/project_settings.rs`` and the contract suites'
``app_project/test_project_settings.py`` arms.
"""

RUST_SETTINGS_DEFAULTS = {
    "agent_stages": {"mode": "auto", "review": True, "test": True},
    "issue_run_policy": "creator_and_assignees",
}

# known_body_diffs entry for a response whose top-level object is the project.
SETTINGS_DEFAULTS_PIN = ("settings", "NEWPIDASH-278", {}, RUST_SETTINGS_DEFAULTS)

# payload_value_diffs entry for the project webhook's before-image: Rust
# serialises ``current_instance`` from the same rendered project, so the
# defaults ride along there too (session-level; unresolved pointers skip).
SETTINGS_DEFAULTS_JOB_PINS = {
    "pi_dash.bgtasks.webhook_task.model_activity": [
        (
            "kwargs/current_instance/settings",
            "NEWPIDASH-278",
            {},
            RUST_SETTINGS_DEFAULTS,
        ),
    ],
}
