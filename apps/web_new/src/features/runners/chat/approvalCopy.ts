// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Wording for the built-in engine's approvals and availability
// (NEWFRONT-553, RUN-033-035). Kept in one pure module so the prompt, the
// mode control and the contact entry all speak with the same voice.

import type { ChatApprovalKind } from "@pidash/api-client";

/** The three modes, most cautious first. */
export const APPROVAL_MODES = ["ask", "workspace", "full_access"] as const;
export type ApprovalMode = (typeof APPROVAL_MODES)[number];

/** One mode's button label and the sentence behind its tooltip. */
export interface ApprovalModeCopy {
  label: string;
  help: string;
}

export const APPROVAL_MODE_COPY: Record<ApprovalMode, ApprovalModeCopy> = {
  ask: {
    label: "Ask every time",
    help: "Approve each command, edit and network call before it runs.",
  },
  workspace: {
    label: "Working copy",
    help: "Edits inside the working copy run freely; anything outside still asks.",
  },
  full_access: {
    label: "Full access",
    help: "Run everything without asking. Commands the engine blocks stay blocked.",
  },
};

/** True for a value the mode control accepts. */
export function isApprovalMode(value: unknown): value is ApprovalMode {
  return typeof value === "string" && (APPROVAL_MODES as readonly string[]).includes(value);
}

/** Read a stored mode, falling back to full access for anything unfamiliar. */
export function readApprovalMode(value: string | null | undefined): ApprovalMode {
  return isApprovalMode(value) ? value : "full_access";
}

/** The storage key a runner's mode lives under. */
export const APPROVAL_MODE_KEY_PREFIX = "pidash:chat-approval-mode:";

export function approvalModeKey(runnerId: string): string {
  return `${APPROVAL_MODE_KEY_PREFIX}${runnerId}`;
}

/** The slice of Storage this module needs; injectable for tests. */
export type ApprovalModeStorage = Pick<Storage, "getItem" | "setItem"> | undefined;

/**
 * Read the mode a runner was last set to. Browsers refuse storage in
 * private or locked-down profiles, so an unreadable profile reads as the
 * engine's default instead of failing the page.
 */
export function storedApprovalMode(
  runnerId: string,
  storage: ApprovalModeStorage = globalThis.localStorage
): ApprovalMode {
  try {
    return readApprovalMode(storage?.getItem(approvalModeKey(runnerId)));
  } catch {
    return "full_access";
  }
}

/** Persist a runner's mode; a refused write leaves the in-memory choice in effect. */
export function rememberApprovalMode(
  runnerId: string,
  mode: ApprovalMode,
  storage: ApprovalModeStorage = globalThis.localStorage
): void {
  try {
    storage?.setItem(approvalModeKey(runnerId), mode);
  } catch {
    /* storage can be unavailable; the in-memory value still applies */
  }
}

/** Sentences for the server's refusal codes on the bundled engine. */
const UNAVAILABLE_REASONS: Record<string, string> = {
  managed_runner_disabled: "The built-in agent is turned off on this server.",
  byok_not_supported_on_desktop: "This computer's built-in agent cannot use your own model key yet.",
  llm_config_missing: "This server has no model the built-in agent can use. Set one up in the workspace settings.",
  desktop_not_connected: "The built-in agent is not connected. Open the desktop app and sign in.",
  desktop_session_required: "Sign out and sign back in through the desktop app to use the built-in agent.",
  gateway_scopes_missing: "Sign out and sign in again so the built-in agent can reach the model gateway.",
  gateway_session_revoked: "Sign in again to reconnect the built-in agent.",
  gateway_unavailable: "The model gateway is briefly unavailable. The built-in agent retries by itself.",
};

/** Human copy for a refusal code; unknown codes surface verbatim. */
export function unavailableReasonText(code: string): string {
  return UNAVAILABLE_REASONS[code] ?? code;
}

/** The activity sentence for one kind of approval request. */
export const APPROVAL_KIND_TEXT: Record<ChatApprovalKind, string> = {
  command_execution: "wants to run a command",
  file_change: "wants to change a file",
  network_access: "wants to reach the network",
  other: "wants your permission",
};

/** Read a kind string as the contract's vocabulary, anything else as other. */
export function approvalKindOf(kind: string): ChatApprovalKind {
  return kind === "command_execution" || kind === "file_change" || kind === "network_access" ? kind : "other";
}

/** One parked approval, as the prompt needs it. */
export interface PendingApproval {
  localApprovalId: string;
  kind: ChatApprovalKind;
  reason: string;
  payload: Record<string, unknown>;
  expiresAt: string | null;
}

function textOf(value: unknown): string | null {
  return typeof value === "string" && value !== "" ? value : null;
}

/**
 * The narrowest thing the request names, for the prompt's headline: a
 * command, then a path, then an address or host.
 */
export function approvalTarget(request: PendingApproval): string | null {
  const payload = request.payload;
  return (
    textOf(payload["command"]) ??
    textOf(payload["path"]) ??
    textOf(payload["url"]) ??
    textOf(payload["destination"]) ??
    textOf(payload["host"])
  );
}

/** Project a translated `chat_approval_request` frame onto the prompt's shape. */
export function pendingApprovalFromEvent(payload: Record<string, unknown>): PendingApproval | null {
  const localApprovalId = payload["local_approval_id"];
  if (typeof localApprovalId !== "string") return null;
  const rest: Record<string, unknown> = { ...payload };
  delete rest["local_approval_id"];
  delete rest["approval_kind"];
  delete rest["reason"];
  delete rest["expires_at"];
  return {
    localApprovalId,
    kind: approvalKindOf(typeof payload["approval_kind"] === "string" ? payload["approval_kind"] : ""),
    reason: typeof payload["reason"] === "string" ? payload["reason"] : "",
    payload: rest,
    expiresAt: typeof payload["expires_at"] === "string" ? payload["expires_at"] : null,
  };
}

/** Milliseconds until a request's deadline; null when it has none or is unreadable. */
export function approvalRemainingMs(request: PendingApproval, nowMs: number): number | null {
  if (request.expiresAt === null) return null;
  const deadline = Date.parse(request.expiresAt);
  if (!Number.isFinite(deadline)) return null;
  return deadline - nowMs;
}
