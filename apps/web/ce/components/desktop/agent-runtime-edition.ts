/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Edition seam for the desktop agent runtime (desktop-overlay
 * `core/services/agent-runtime.ts`). Editions replace this file wholesale —
 * keep both export names stable.
 */

/** Endpoint the agent runtime fetches a CSRF token from before unsafe calls. */
export const CSRF_TOKEN_PATH = "/auth/get-csrf-token/";

/**
 * User-facing text for the `reason_code` / `error` values the managed-runner
 * endpoints return (`pi_dash.managed_runner.errors.ManagedRunnerReason`).
 * Codes without an entry are shown as-is.
 */
export const AGENT_RUNTIME_REASON_MESSAGES: Record<string, string> = {
  managed_runner_disabled: "Pi Dash Agent is not enabled on this server.",
  byok_not_supported_on_desktop:
    "Pi Dash Agent can't run on this computer with your own API key yet. Pi Dash AI and cloud runs keep using your key.",
  llm_config_missing: "This server has no model the desktop agent can use. Configure one in Settings → AI Assistant.",
  gateway_session_revoked: "Sign in again to reconnect Pi Dash Agent.",
  desktop_session_required: "Sign out and sign in through the desktop app to connect Pi Dash Agent.",
};
