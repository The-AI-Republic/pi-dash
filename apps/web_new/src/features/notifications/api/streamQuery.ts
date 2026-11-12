// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reading a cached stream back out of its query key. Card writes walk every
// cached page through this so a card moves between the streams that follow
// its new flags (parity/specs/notifications.md §9). It lives apart from the
// key builders because the shell imports those eagerly for the unread dot;
// only the card-action layer needs this decoder, so keeping it out of that
// module keeps the eager entry chunk unchanged.
import type { NotificationListQuery } from "@pidash/api-client";

import { notificationKeys } from "./keys.js";

/**
 * Rebuild one stream query from a list cache key, or null when the key is
 * not a list key of this workspace. Card mutations walk every cached page
 * through this to apply (or roll back) a row change per stream — an
 * archived card stays in the archived stream and a snoozed card stays in
 * the snoozed one — instead of refetching every stream on each write.
 */
export function notificationListQueryFromKey(
  workspaceSlug: string,
  key: readonly unknown[]
): NotificationListQuery | null {
  const root = notificationKeys.lists(workspaceSlug);
  // The key is the lists root (which already ends in "list") followed by
  // the five query parts and, for the cursor stream, a trailing "pages".
  const isPages = key.length === root.length + 6 && key[root.length + 5] === "pages";
  if (!isPages && key.length !== root.length + 5) return null;
  if (root.some((part, index) => key[index] !== part)) return null;
  const [type, snoozed, archived, read, mentioned] = key.slice(root.length) as [
    string | null,
    boolean | null,
    boolean | null,
    boolean | null,
    boolean,
  ];
  const query: NotificationListQuery = {};
  if (type !== null) query.type = type;
  if (snoozed !== null) query.snoozed = snoozed;
  if (archived !== null) query.archived = archived;
  if (read !== null) query.read = read;
  if (mentioned) query.mentioned = true;
  return query;
}
