// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Why the desktop agent cannot run, in words that name the fix
// (parity/specs/cloud.md §8, CLOUD-040). The desktop engine runs on the
// hosted model lane only, so each refusal has its own situation and its
// own sentence: an operator switch, the visitor's own key (which still
// serves cloud runs), a missing model, a revoked gateway session, a
// browser session where the desktop flow is required, an app that was
// never connected, stale gateway permissions, and a gateway outage.
// Related codes never share an entry — do not collapse them.

export const AGENT_REASON_MESSAGES: Readonly<Record<string, string>> = {
  managed_runner_disabled:
    "Pi Dash Agent is switched off on this server. An instance administrator can turn it back on.",
  byok_not_supported_on_desktop:
    "This computer runs Pi Dash Agent on the hosted model lane. Choose that lane in AI Assistant settings; the key you supplied still serves cloud runs.",
  llm_config_missing: "Choose a model in AI Assistant settings before running Pi Dash Agent on this computer.",
  gateway_session_revoked: "The model gateway session was revoked. Sign in again to reconnect Pi Dash Agent.",
  desktop_session_required:
    "Pi Dash Agent only runs inside the desktop app. Sign out and sign in through the desktop app to connect it.",
  desktop_not_connected:
    "The desktop app is not connected yet. Open the desktop app and sign in there to connect Pi Dash Agent.",
  gateway_scopes_missing:
    "The model gateway has not granted the permissions Pi Dash Agent needs. Sign out and sign in again to refresh them.",
  gateway_unavailable:
    "The model gateway is temporarily unavailable. Pi Dash Agent retries on its own once it answers again.",
};

/** The reason codes this build explains, in a stable order. */
export const AGENT_REASON_CODES = Object.keys(AGENT_REASON_MESSAGES) as readonly string[];

/**
 * The sentence for a server code. An unknown code stays visible as code
 * text: naming it is better than claiming a failure with no explanation.
 */
export function agentReasonMessage(code: string): string {
  return AGENT_REASON_MESSAGES[code] ?? code;
}

/**
 * True when a refusal means "sign in again" rather than "retry later".
 * A revoked gateway session and stale scopes are the two the engine can
 * only fix with a new sign-in.
 */
/**
 * Read the reason code off a refusal from any managed-runner endpoint:
 * profile and availability gates answer `{error, detail}`, while the
 * credential mint answers the same shape (409/401/503). A transport or
 * contract failure carries no code, so it stays null and the caller
 * surfaces the real error instead of inventing a reason.
 */
export function agentRefusalCode(body: unknown): string | null {
  if (typeof body !== "object" || body === null) return null;
  const code = (body as { error?: unknown }).error;
  return typeof code === "string" && code.length > 0 ? code : null;
}

/**
 * True when a refusal code is one this build explains. A code outside the
 * map is still shown verbatim by agentReasonMessage; this only decides
 * whether the caller replaces the failure with a named EngineUnavailable.
 */
export function isKnownAgentReason(code: string): boolean {
  return Object.hasOwn(AGENT_REASON_MESSAGES, code);
}

export function agentReasonNeedsSignIn(code: string): boolean {
  return code === "gateway_session_revoked" || code === "gateway_scopes_missing";
}
