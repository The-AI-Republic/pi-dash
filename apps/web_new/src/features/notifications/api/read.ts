// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// First-open read mutation (parity/specs/notifications.md §5, NTF-007).
// The server answer contributes only read_at: mutation echoes omit stream
// annotations, so replacing the whole cached row would lose its tab data.
import { useMutation, useQueryClient, type InfiniteData, type QueryClient } from "@tanstack/react-query";
import {
  markNotificationRead,
  type NotificationItem,
  type NotificationListResponse,
  type UnreadNotifications,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notificationKeys } from "./keys.js";

export interface MarkNotificationReadInput {
  id: string;
  mentioned: boolean;
}

function patchPage(page: NotificationListResponse, notificationId: string, readAt: string): NotificationListResponse {
  return {
    ...page,
    results: page.results.map((row) => (row.id === notificationId ? { ...row, read_at: readAt } : row)),
  };
}

function isNotificationPage(value: unknown): value is NotificationListResponse {
  return typeof value === "object" && value !== null && Array.isArray((value as { results?: unknown }).results);
}

/** Patch either the legacy first-page cache or the cursor-page cache. */
export function applyReadToNotificationCache(current: unknown, notificationId: string, readAt: string): unknown {
  if (isNotificationPage(current)) return patchPage(current, notificationId, readAt);
  if (typeof current !== "object" || current === null) return current;
  const pages = (current as { pages?: unknown }).pages;
  if (!Array.isArray(pages) || !pages.every(isNotificationPage)) return current;
  const data = current as InfiniteData<NotificationListResponse, string>;
  return { ...data, pages: data.pages.map((page) => patchPage(page, notificationId, readAt)) };
}

/** A read notification lowers the total and, for mentions, the mention total. */
export function applyReadToTotals(
  current: UnreadNotifications | undefined,
  mentioned: boolean
): UnreadNotifications | undefined {
  if (current === undefined) return undefined;
  return {
    total_unread_notifications_count: Math.max(0, current.total_unread_notifications_count - 1),
    mention_unread_notifications_count: mentioned
      ? Math.max(0, current.mention_unread_notifications_count - 1)
      : current.mention_unread_notifications_count,
  };
}

export function applySuccessfulRead(
  queryClient: QueryClient,
  workspaceSlug: string,
  updated: NotificationItem,
  mentioned: boolean
): void {
  if (updated.read_at === null) return;
  queryClient.setQueriesData({ queryKey: notificationKeys.lists(workspaceSlug) }, (current) =>
    applyReadToNotificationCache(current, updated.id, updated.read_at as string)
  );
  queryClient.setQueryData<UnreadNotifications>(notificationKeys.unread(workspaceSlug), (current) =>
    applyReadToTotals(current, mentioned)
  );
}

/** Background read write used by card selection. */
export function useMarkNotificationRead(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: MarkNotificationReadInput) => markNotificationRead(getAppClient(), workspaceSlug, input.id),
    onSuccess: (updated, input) => {
      applySuccessfulRead(queryClient, workspaceSlug, updated, input.mentioned);
    },
  });
}
