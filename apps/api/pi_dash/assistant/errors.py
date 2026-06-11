# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Typed error taxonomy for the assistant.

See ``.ai_design/integrate_ai_agent/02-backend.md`` §9.3. Codes surface either
as a synchronous HTTP error on the REST endpoints or as a ``turn_failed`` event
paired with an ``error`` message row when a turn fails asynchronously.
"""

from __future__ import annotations


class AssistantError(Exception):
    """Base for assistant errors carrying a stable machine code."""

    code = "internal"
    http_status = 500

    def __init__(self, detail: str = "", *, code: str | None = None, http_status: int | None = None):
        super().__init__(detail or self.__class__.__name__)
        self.detail = detail
        if code is not None:
            self.code = code
        if http_status is not None:
            self.http_status = http_status


class LLMConfigMissing(AssistantError):
    code = "llm_config_missing"
    http_status = 422


class AssistantNotConfigured(AssistantError):
    code = "assistant_not_configured"
    http_status = 503


class RoleNotAllowed(AssistantError):
    code = "role_not_allowed"
    http_status = 403


class TurnActive(AssistantError):
    code = "turn_active"
    http_status = 409


class ThreadFull(AssistantError):
    code = "thread_full"
    http_status = 409


class QuotaExceeded(AssistantError):
    code = "quota_exceeded"
    http_status = 402


class BaseUrlBlocked(AssistantError):
    code = "base_url_blocked"
    http_status = 400


# --- Turn-runtime failure codes (surface as turn_failed events) ---

class ProviderAuthFailed(AssistantError):
    code = "provider_auth_failed"
    http_status = 502


class ProviderUnreachable(AssistantError):
    code = "provider_unreachable"
    http_status = 502


class ModelInvalid(AssistantError):
    code = "model_invalid"
    http_status = 400


class TurnTimeout(AssistantError):
    code = "turn_timeout"
    http_status = 504


class IterationLimit(AssistantError):
    code = "iteration_limit"
    http_status = 400


# Thread/message size limits
MAX_THREAD_MESSAGES = 200
MAX_MESSAGE_CHARS = 32_000
