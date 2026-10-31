// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inbox stream controls (written from parity/specs/notifications.md §2,
// §7, §8 and §11, NTF-002, NTF-013-017, NTF-028). Tabs stay slice A's;
// this module owns what narrows a tab's stream: the origin filter (a
// multi-select of assigned/created/subscribed, joined in that fixed
// order), the three overflow modes (unread-only, archived, snoozed, with
// archived and snoozed excluding each other) and the list query the
// server sees for each combination. Every state change derives a new
// query, which is also the restart signal: each query is its own first
// page (continuations belong to the pagination slice).
import type { NotificationItem, NotificationListQuery } from "@pidash/api-client";

/** The two tabs: the full stream and the mentions-only stream. */
export type NotificationTab = "all" | "mentions";

/** Tab order: the full stream first, mentions second. */
export const NOTIFICATION_TABS: readonly NotificationTab[] = ["all", "mentions"] as const;

/** List query for one tab under the default (unfiltered, no-mode) stream. */
export function notificationTabQuery(tab: NotificationTab): NotificationListQuery {
  return notificationStreamQuery(tab, [], DEFAULT_STREAM_MODES);
}

/** Inbox origins, in the order they join the type parameter. */
export type NotificationOrigin = "assigned" | "created" | "subscribed";

/** Origin order: assigned first, then created, then subscribed. */
export const NOTIFICATION_ORIGINS: readonly NotificationOrigin[] = ["assigned", "created", "subscribed"] as const;

/** Wording for each origin, shown in the menu and on its chip. */
export const NOTIFICATION_ORIGIN_LABELS: Record<NotificationOrigin, string> = {
  assigned: "Assigned to me",
  created: "Created by me",
  subscribed: "I'm subscribed",
};

/** The active origin filter, held in canonical order. */
export type OriginFilter = readonly NotificationOrigin[];

/** Overflow modes; unread-only is independent of the archive/snooze pair. */
export interface StreamModes {
  unreadOnly: boolean;
  archived: boolean;
  snoozed: boolean;
}

export const DEFAULT_STREAM_MODES: StreamModes = { unreadOnly: false, archived: false, snoozed: false };

/** The overflow menu's rows, in menu order. */
export type StreamMode = "unread" | "archived" | "snoozed";

export const NOTIFICATION_MODES: readonly StreamMode[] = ["unread", "archived", "snoozed"] as const;

/** Menu wording for each mode. */
export const NOTIFICATION_MODE_LABELS: Record<StreamMode, string> = {
  unread: "Show unread only",
  archived: "Show archived",
  snoozed: "Show snoozed",
};

const MODE_KEYS: Record<StreamMode, keyof StreamModes> = {
  unread: "unreadOnly",
  archived: "archived",
  snoozed: "snoozed",
};

/** Whether one mode is currently on. */
export function isModeEnabled(modes: StreamModes, mode: StreamMode): boolean {
  return modes[MODE_KEYS[mode]];
}

/**
 * Add or remove one origin, keeping the canonical order. The result is
 * always a fresh array so a change is visible by identity.
 */
export function toggleOrigin(filter: OriginFilter, origin: NotificationOrigin): NotificationOrigin[] {
  const active = new Set(filter);
  if (active.has(origin)) {
    active.delete(origin);
  } else {
    active.add(origin);
  }
  return NOTIFICATION_ORIGINS.filter((candidate) => active.has(candidate));
}

/** Remove one origin (a chip's delete action). */
export function removeOrigin(filter: OriginFilter, origin: NotificationOrigin): NotificationOrigin[] {
  return filter.filter((candidate) => candidate !== origin);
}

/**
 * Flip one overflow mode. Turning archived on turns snoozed off and the
 * other way round; unread-only is untouched by either.
 */
export function toggleMode(modes: StreamModes, mode: StreamMode): StreamModes {
  const next: StreamModes = { ...modes, [MODE_KEYS[mode]]: !isModeEnabled(modes, mode) };
  if (mode === "archived" && next.archived) {
    next.snoozed = false;
  } else if (mode === "snoozed" && next.snoozed) {
    next.archived = false;
  }
  return next;
}

/**
 * The list query for one tab under the current filter and modes. Active
 * origins join as the comma-separated type parameter (absent when none
 * are active); unread-only sets read false and read is absent otherwise;
 * the default stream sends both exclusion flags false. Mentions stays a
 * flag on top of the same narrowed stream.
 */
export function notificationStreamQuery(
  tab: NotificationTab,
  origins: OriginFilter,
  modes: StreamModes
): NotificationListQuery {
  const query: NotificationListQuery = {
    snoozed: modes.snoozed,
    archived: modes.archived,
  };
  if (origins.length > 0) {
    query.type = origins.join(",");
  }
  if (modes.unreadOnly) {
    query.read = false;
  }
  if (tab === "mentions") {
    query.mentioned = true;
  }
  return query;
}

/**
 * Mark-all-read scope: the current list scope as a request body. The
 * origin, snoozed and archived flags ride along; the read flag does not,
 * because bulk read only ever targets unread rows.
 */
export function markAllReadScope(
  origins: OriginFilter,
  modes: StreamModes
): { type?: string; snoozed: boolean; archived: boolean } {
  return {
    ...(origins.length === 0 ? {} : { type: origins.join(",") }),
    snoozed: modes.snoozed,
    archived: modes.archived,
  };
}

/**
 * Whether a fetched row is hidden from the stream it arrived in. The
 * server ships future-snoozed rows on the default query, so the default
 * stream hides them on the client (NTF-017); the archived and snoozed
 * modes show their rows as the server sent them.
 */
export function isHiddenByStreamMode(row: NotificationItem, modes: StreamModes, nowMs: number): boolean {
  if (modes.archived || modes.snoozed) return false;
  return row.snoozed_till !== null && Date.parse(row.snoozed_till) > nowMs;
}
