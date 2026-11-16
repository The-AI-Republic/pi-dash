// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Refusal helpers for the local agent runtime (parity/specs/cloud.md §8,
// CLOUD-040). The sentences themselves live in the shared agent wording
// map so the runtime, the status notice and the picker never explain the
// same server code two different ways; this module only reads codes and
// options out of a failed agent call.

import { AGENT_REASON_MESSAGES } from "../../../shared/lib/agentReasons.js";

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
 * map is still shown verbatim; this only decides whether the caller
 * replaces the failure with a named EngineUnavailable.
 */
export function isKnownAgentReason(code: string): boolean {
  return Object.hasOwn(AGENT_REASON_MESSAGES, code);
}
