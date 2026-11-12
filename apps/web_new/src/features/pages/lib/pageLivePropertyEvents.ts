// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page state over the collaboration channel (parity/specs/pages.md §2.3,
// §3.8, PAGE-058). Locking is a cooperative freeze: the acting client
// writes the lock endpoint, then tells the page's live socket so every
// other viewer freezes without another read. The live mount relays one
// bare server name ("lock") to every socket on the document as the
// matching client name ("locked"); this client sends the former and
// applies the latter to its caches, swallowing its own echo. Only the
// lock pair rides here: no scenario pins peer-observed archive,
// visibility or delete, so those broadcasts stay ignored.

import type { QueryClient } from "@tanstack/react-query";
import type { PageRow as PageRowData } from "@pidash/api-client";

import { PAGE_EVENT_LOCKED, PAGE_EVENT_UNLOCKED, patchPageRow } from "../api/rowMutations.js";

/** The client names this side sends and receives (PAGE-058). */
export type PageLivePropertyEvent = typeof PAGE_EVENT_LOCKED | typeof PAGE_EVENT_UNLOCKED;

/** The wire names the live mount relays back as client names. */
export type PageLivePropertyWire = "lock" | "unlock";

/** Local event to the bare wire name the mount understands. */
const PROPERTY_WIRE: Record<PageLivePropertyEvent, PageLivePropertyWire> = {
  [PAGE_EVENT_LOCKED]: "lock",
  [PAGE_EVENT_UNLOCKED]: "unlock",
};

/**
 * Read one name — a local page event or an inbound stateless payload —
 * as a property event. The mount relays bare client names, so anything
 * else (a JSON error frame, a force-close notice, another feature's
 * broadcast) is not one.
 */
export function parsePageLiveProperty(value: string): PageLivePropertyEvent | null {
  if (value === PAGE_EVENT_LOCKED || value === PAGE_EVENT_UNLOCKED) return value;
  return null;
}

/** One property event's bare wire name for the mount. */
export function pageLivePropertyWire(event: PageLivePropertyEvent): PageLivePropertyWire {
  return PROPERTY_WIRE[event];
}

/**
 * Sends awaiting their echo, per page, oldest first. The mount relays
 * every send back to its own socket too, so without suppression a fast
 * lock-then-unlock would land its stale "locked" echo after the local
 * unlock and freeze a page the viewer just freed.
 */
const pendingEchoes = new Map<string, PageLivePropertyEvent[]>();

/** How many unanswered sends one page keeps before the oldest is dropped. */
const MAX_PENDING_ECHOES = 16;

/** Tests only: forget every pending echo so a suite starts clean. */
export function resetPageLivePropertyEchoes(): void {
  pendingEchoes.clear();
}

/** Drop one page's pending echoes (its socket died or its session ended). */
export function clearPageLivePropertyEchoes(pageId: string): void {
  pendingEchoes.delete(pageId);
}

/** Remember one send whose relay is still on the wire. */
export function notePageLivePropertySent(pageId: string, event: PageLivePropertyEvent): void {
  const pending = pendingEchoes.get(pageId) ?? [];
  pending.push(event);
  while (pending.length > MAX_PENDING_ECHOES) pending.shift();
  pendingEchoes.set(pageId, pending);
}

/**
 * Swallow one inbound event when it is the oldest unanswered send's
 * echo. Anything else — a peer's action, a duplicate, an echo with no
 * send behind it — is genuine and must apply.
 */
export function takePageLivePropertyEcho(pageId: string, event: PageLivePropertyEvent): boolean {
  const pending = pendingEchoes.get(pageId);
  if (pending === undefined || pending[0] !== event) return false;
  pending.shift();
  if (pending.length === 0) pendingEchoes.delete(pageId);
  return true;
}

/**
 * Apply one peer property event to the cached detail row and collection
 * row, without another read: the sender already persisted it. Rows for
 * other pages keep their identity; absent caches stay absent.
 */
export function applyPageLiveProperty(
  queryClient: QueryClient,
  detailKey: readonly unknown[],
  listKey: readonly unknown[],
  pageId: string,
  event: PageLivePropertyEvent
): void {
  const patch = event === PAGE_EVENT_LOCKED ? { is_locked: true } : { is_locked: false };
  queryClient.setQueryData<PageRowData | undefined>(detailKey, (row) =>
    row === undefined || row.id !== pageId ? row : { ...row, ...patch }
  );
  queryClient.setQueryData<PageRowData[] | undefined>(listKey, (rows) => patchPageRow(rows, pageId, patch));
}
