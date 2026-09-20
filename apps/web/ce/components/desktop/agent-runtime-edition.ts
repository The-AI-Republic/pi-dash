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
 * endpoints return. Codes without an entry are shown as-is.
 *
 * These codes come from two backend families, not one — keep both covered:
 *   - `pi_dash.managed_runner.errors.ManagedRunnerReason` (the profile
 *     endpoint's `reason_code` and the availability gates): e.g.
 *     `managed_runner_disabled`, `desktop_not_connected`, `llm_config_missing`,
 *     `gateway_scopes_missing`, `byok_not_supported_on_desktop`.
 *   - The credential/permission literals: `gateway_session_revoked` and
 *     `gateway_unavailable` from `assistant/views/agent_profile.py`, and
 *     `desktop_session_required` from `managed_runner/permissions.py`.
 * `gateway_scopes_missing` and `gateway_session_revoked` (likewise
 * `desktop_not_connected` and `desktop_session_required`) are related but
 * distinct situations — do not collapse them into one key.
 *
 * `agent-runtime-reason-codes.test.ts` fails if this map and the backend codes
 * drift apart.
 */
export const AGENT_RUNTIME_REASON_MESSAGES: Record<string, string> = {
  managed_runner_disabled: "Pi Dash Agent is not enabled on this server.",
  byok_not_supported_on_desktop:
    "Pi Dash Agent can't run on this computer with your own API key yet. Pi Dash AI and cloud runs keep using your key.",
  llm_config_missing: "This server has no model the desktop agent can use. Configure one in Settings → AI Assistant.",
  desktop_not_connected: "Pi Dash Agent isn't connected. Open and sign in to the desktop app to connect it.",
  desktop_session_required: "Sign out and sign in through the desktop app to connect Pi Dash Agent.",
  gateway_scopes_missing: "Sign out and sign in again to give Pi Dash Agent access to the model gateway.",
  gateway_session_revoked: "Sign in again to reconnect Pi Dash Agent.",
  gateway_unavailable: "The model gateway is temporarily unavailable. Pi Dash Agent will retry automatically.",
};
