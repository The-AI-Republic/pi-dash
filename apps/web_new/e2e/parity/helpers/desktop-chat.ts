// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop chat legs (DESK-009) send into a conversation the on-machine store
// actually holds and prove the turn they sent. Both are judgments about
// recorded state rather than about the page's paint, so they live here and
// the specs stay readable: an address can outlive the conversation it names
// (a clear removes every row), and a cumulative append count lets an earlier
// leg's turn stand in for one that stored nothing.
import type { DesktopInvocation } from "../desktop/tauri-mock.js";

/**
 * The session a leg should send into: the address's conversation when the
 * store still carries it, otherwise nothing — the caller picks a stored row
 * and the address follows it.
 */
export function sessionToSendInto(addressId: string | null, storedIds: readonly string[]): string | null {
  if (addressId === null) return null;
  return storedIds.includes(addressId) ? addressId : null;
}

/** How many `chat_append_event` invokes recorded this exact user turn for this session. */
export function appendedTurns(invocations: readonly DesktopInvocation[], sessionId: string, text: string): number {
  return invocations.filter((invoke) => {
    if (invoke.command !== "chat_append_event") return false;
    const args = invoke.args as
      | { sessionId?: unknown; event?: { role?: unknown; content?: unknown } }
      | null
      | undefined;
    if (args === null || args === undefined) return false;
    return args.sessionId === sessionId && args.event?.role === "user" && args.event?.content === text;
  }).length;
}

/** How many invokes of one command were recorded. */
export function invocationCount(invocations: readonly DesktopInvocation[], command: string): number {
  return invocations.filter((invoke) => invoke.command === command).length;
}
