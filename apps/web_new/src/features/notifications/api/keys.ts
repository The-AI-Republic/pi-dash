// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Notification query keys (Data layer; written from
// parity/specs/notifications.md §1-§4, NTF-001-004). Workspace-scoped keys
// start with the workspace scope so a workspace switch or sign-out clears
// one subtree; the list keys carry the full stream query so each tab (and
// later each filter/mode combination) caches its own first page. The email
// preferences are the caller's own settings — the same in every workspace —
// so they hang off a user-level root instead (sign-out clears the whole
// cache, so no stale user survives). The shell reads only the unread count
// through these.
import type { NotificationListQuery } from "@pidash/api-client";

import { workspaceScope } from "../../../core/query/client.js";

export const notificationKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "notifications"] as const,
  unread: (workspaceSlug: string) => [...notificationKeys.all(workspaceSlug), "unread"] as const,
  /** The caller's email delivery preferences (user-level, no workspace). */
  emailPreferences: ["notifications", "email-preferences"] as const,
  lists: (workspaceSlug: string) => [...notificationKeys.all(workspaceSlug), "list"] as const,
  list: (workspaceSlug: string, query: NotificationListQuery) =>
    [
      ...notificationKeys.lists(workspaceSlug),
      query.type ?? null,
      query.snoozed ?? null,
      query.archived ?? null,
      query.read ?? null,
      // The view truthiness-checks the raw mentioned string, so false and
      // absent are the same stream server-side; key them as one entry.
      query.mentioned === true,
    ] as const,
};

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
  // the five query parts.
  if (key.length !== root.length + 5) return null;
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
