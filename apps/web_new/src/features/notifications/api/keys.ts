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
  pages: (workspaceSlug: string, query: NotificationListQuery) =>
    [...notificationKeys.list(workspaceSlug, query), "pages"] as const,
};
