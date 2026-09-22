# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Stable, user-facing reason codes for the managed runner.

Every refusal carries one of these codes rather than free text so the web
picker, the desktop app and the issue serializer all render the same
explanation for the same situation.
"""

from __future__ import annotations


class ManagedRunnerReason:
    """Reason codes, in the precedence order ``managed_runner_availability``
    evaluates them. The first failing gate wins, which is what makes the copy
    deterministic for a given user/project pair."""

    #: The operator kill switch is off for this instance.
    DISABLED = "managed_runner_disabled"
    #: No viewer supplied, or the viewer's desktop app is not connected.
    NOT_CONNECTED = "desktop_not_connected"
    #: The viewer has no usable LLM configuration at all.
    LLM_CONFIG_MISSING = "llm_config_missing"
    #: The viewer's session predates the gateway scopes and cannot acquire them.
    GATEWAY_SCOPES_MISSING = "gateway_scopes_missing"
    #: The viewer's provider is BYOK, which the desktop engine does not serve
    #: in the MVP (no stored key is ever returned to a client).
    BYOK_UNSUPPORTED = "byok_not_supported_on_desktop"
    #: The desktop is connected but has not enrolled a runner for this project
    #: yet. The desktop resolves this silently by enrolling and retrying.
    NO_RUNNER_FOR_PROJECT = "no_managed_runner_for_project"


class ManagedRunnerUnavailable(ValueError):
    """Raised at creation when a managed run cannot be admitted.

    ``code`` is a :class:`ManagedRunnerReason` value; ``str(exc)`` is the
    human-readable sentence for API responses.
    """

    def __init__(self, code: str, detail: str = ""):
        self.code = code
        super().__init__(detail or code)
