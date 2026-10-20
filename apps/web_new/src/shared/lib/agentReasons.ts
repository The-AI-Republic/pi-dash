// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Human wording for the server's agent availability vocabulary
// (parity/specs/desktop.md §1 DESK-004, §7, §14). The codes are API
// contract and travel untouched; only the sentences live here, written
// for this tree. Every consumer that shows a refusal — the project status
// notice, the execution-target picker, the chat entry — reads one map so
// the same situation never gets two explanations. Unknown codes surface
// as-is, and nothing here throws: a refusal must never replace the error
// that caused it with a crash.

import { isApiError } from "@pidash/api-client";

/**
 * One plain-language sentence per server or client reason code. Each
 * refusal names the fix: an operator switch, the visitor's own key (which
 * still serves cloud runs), a missing model, a revoked gateway session, a
 * browser session where the desktop flow is required, an app that was
 * never connected, stale gateway permissions, and a gateway outage.
 */
export const AGENT_REASON_MESSAGES: Readonly<Record<string, string>> = {
  // Agent profile and machine gates (managed_runner/errors.py order).
  managed_runner_disabled:
    "Pi Dash Agent is switched off on this server. An instance administrator can turn it back on.",
  desktop_not_connected:
    "The desktop app is not connected yet. Open the desktop app and sign in there to connect Pi Dash Agent.",
  // Both the Cloud Agent row and the built-in agent row report this code
  // (pi_dash.core.agent_execution): the viewer has no model configured, so
  // the sentence names neither surface.
  llm_config_missing: "Choose a model in AI Assistant settings before running Pi Dash Agent on this computer.",
  gateway_scopes_missing:
    "Your sign-in is missing permission to reach the model gateway. Sign out, then sign in again to refresh it.",
  byok_not_supported_on_desktop:
    "This computer runs Pi Dash Agent on the hosted model lane. Choose that lane in AI Assistant settings; the provider key you supplied still serves cloud runs.",
  no_managed_runner_for_project: "This computer has not enrolled a runner for this project yet.",

  // Credential and permission literals from the profile and token endpoints.
  desktop_session_required:
    "Pi Dash Agent only runs inside the desktop app. Sign out and sign in through the desktop app to connect it.",
  gateway_session_revoked: "The model gateway session was revoked. Sign in again to reconnect Pi Dash Agent.",
  gateway_unavailable:
    "The model gateway is temporarily unavailable. Pi Dash Agent retries on its own once it answers again.",

  // Client-side marker for a profile read that never answered.
  agent_profile_unreachable: "Could not reach the server to check whether the built-in agent can run.",

  // Other execution targets the same picker renders.
  cloud_agent_unavailable: "The Cloud Agent is switched off on this server.",
  no_local_runner: "No online runner is available on this project's pods.",
};

/**
 * Render one reason code. Unknown codes are returned unchanged on purpose:
 * the server vocabulary may grow, and showing the raw code is better than
 * hiding the refusal behind a generic sentence.
 */
export function agentReasonMessage(code: string): string {
  return AGENT_REASON_MESSAGES[code] ?? code;
}

/** Read a machine reason code out of a parsed error body, if it carries one. */
function reasonCodeFromBody(body: unknown): string | null {
  if (body === null || typeof body !== "object") return null;
  const record = body as Record<string, unknown>;
  for (const key of ["error", "error_code", "code"]) {
    const value = record[key];
    if (typeof value === "string" && value.length > 0) return value;
  }
  return null;
}

/**
 * Turn a failed agent call into what the user should read. Server refusals
 * arrive as an API error whose body names the reason; those become the
 * mapped sentence. Everything else keeps its own message so transport and
 * programming failures are not disguised as an agent refusal.
 */
export function agentFailureMessage(error: unknown): string {
  if (isApiError(error)) {
    const code = reasonCodeFromBody(error.body);
    if (code !== null) return agentReasonMessage(code);
    return error.message;
  }
  if (error instanceof Error) return error.message;
  return String(error);
}
