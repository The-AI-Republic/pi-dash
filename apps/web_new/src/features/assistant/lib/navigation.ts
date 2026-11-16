// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant addresses (parity/specs/agents.md §13). The landing and every
// conversation keep stable addresses so the shell's assistant card, the
// rail and copied deep links all point at the same place.

/** The assistant landing for one workspace. */
export function assistantLandingHref(workspaceSlug: string): string {
  return `/${workspaceSlug}/assistant`;
}

/** One conversation inside the assistant. */
export function assistantThreadHref(workspaceSlug: string, threadId: string): string {
  return `${assistantLandingHref(workspaceSlug)}/${threadId}`;
}

/** The first message a fresh conversation still has to deliver. */
export interface PendingMessageState {
  pendingMessage: string;
}

/** Navigation state a started conversation carries, or null when it carries none. */
export function readPendingMessage(state: unknown): string | null {
  if (typeof state !== "object" || state === null) return null;
  const pending = (state as { pendingMessage?: unknown }).pendingMessage;
  if (typeof pending !== "string") return null;
  const trimmed = pending.trim();
  return trimmed === "" ? null : trimmed;
}
