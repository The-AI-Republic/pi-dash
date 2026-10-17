// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inbox stream tabs (written from parity/specs/notifications.md §2,
// NTF-002). Exactly two tabs: the full stream, active by default, and the
// mentions-only stream, which is the same list endpoint with the
// mentioned-only flag. Both carry the default stream's explicit
// exclusion flags; filters and modes extend the query, never the tabs.
import type { NotificationListQuery } from "@pidash/api-client";

export type NotificationTab = "all" | "mentions";

/** Tab order: the full stream first, mentions second. */
export const NOTIFICATION_TABS: readonly NotificationTab[] = ["all", "mentions"] as const;

/** List query for one tab's first page. */
export function notificationTabQuery(tab: NotificationTab): NotificationListQuery {
  const query: NotificationListQuery = { snoozed: false, archived: false };
  if (tab === "mentions") {
    query.mentioned = true;
  }
  return query;
}
