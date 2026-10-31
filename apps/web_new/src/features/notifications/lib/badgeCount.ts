// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Mentions-priority badge selection (written from
// parity/specs/notifications.md §3, NTF-003). The notifications area owns
// this rule as a unit-tested data function; the shell renders its
// presence dot off the same unread source, and a future count badge in
// chrome consumes this function with no new seam.
import type { UnreadNotifications } from "@pidash/api-client";

/**
 * The at-a-glance unread count: the mention total while any mention is
 * unread, otherwise the overall total. Unknown totals read as zero so a
 * badge never shows a stale or guessed count.
 */
export function selectUnreadBadgeCount(totals: UnreadNotifications | undefined): number {
  if (totals === undefined) return 0;
  if (totals.mention_unread_notifications_count > 0) return totals.mention_unread_notifications_count;
  return totals.total_unread_notifications_count;
}
