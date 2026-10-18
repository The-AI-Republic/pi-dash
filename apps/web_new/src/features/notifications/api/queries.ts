// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Notification queries (written from parity/specs/notifications.md §1,
// §2, §4, NTF-001-005). The sibling slices build on these two reads: the
// unread totals behind the tab badges (and the shell's inbox dot, whose
// prefetch already lives in the gated layout) and the paginated inbox
// list for one stream. Both stay disabled without a workspace so no
// request fires.
import { infiniteQueryOptions, queryOptions, useInfiniteQuery, useQuery } from "@tanstack/react-query";
import {
  getUnreadNotifications,
  listNotificationsPage,
  type ApiClient,
  type NotificationListResponse,
  type NotificationListQuery,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { notificationKeys } from "./keys.js";

/** Fixed inbox page size (spec §6: 300 rows per page). */
export const NOTIFICATIONS_PAGE_SIZE = 300;

/**
 * Cursor for a fresh stream load: first page at zero offset. Continuations
 * (the explicit next-page control, NTF-010) follow the stored next cursor
 * and belong to the sibling slice.
 */
export const NOTIFICATIONS_FIRST_CURSOR = `${NOTIFICATIONS_PAGE_SIZE}:0:0`;

export function unreadNotificationsQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: notificationKeys.unread(workspaceSlug),
    queryFn: () => getUnreadNotifications(client, workspaceSlug),
    staleTime: LIST_STALE_TIME_MS,
    enabled: workspaceSlug.length > 0,
    // NTF-028: badges move only on an explicit refresh, a tab/filter
    // change or a remount — never on focus, reconnect or a timer.
    refetchOnMount: "always",
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

/** Unread totals for one workspace; components read through this hook. */
export function useUnreadNotifications(workspaceSlug: string) {
  return useQuery(unreadNotificationsQueryOptions(getAppClient(), workspaceSlug));
}

/** First page of one inbox stream (tab, later filter/mode queries too). */
export function notificationsListQueryOptions(client: ApiClient, workspaceSlug: string, query: NotificationListQuery) {
  return queryOptions({
    queryKey: notificationKeys.list(workspaceSlug, query),
    queryFn: () =>
      listNotificationsPage(client, workspaceSlug, {
        ...query,
        perPage: NOTIFICATIONS_PAGE_SIZE,
        cursor: NOTIFICATIONS_FIRST_CURSOR,
      }),
    staleTime: LIST_STALE_TIME_MS,
    enabled: workspaceSlug.length > 0,
    // NTF-028: no push and no focus polling. A remount is one of the three
    // triggers that may re-read, so a warm stream re-reads when the inbox
    // is entered again instead of serving the previous visit's rows.
    refetchOnMount: "always",
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

/** First page of one inbox stream; components read through this hook. */
export function useNotificationsList(workspaceSlug: string, query: NotificationListQuery) {
  return useQuery(notificationsListQueryOptions(getAppClient(), workspaceSlug, query));
}

/** The next opaque cursor, or no continuation once the server is exhausted. */
export function nextNotificationCursor(page: NotificationListResponse): string | undefined {
  return page.next_page_results && page.next_cursor !== "" ? page.next_cursor : undefined;
}

/** All pages of one stream; only an explicit fetchNextPage reads a continuation. */
export function notificationPagesQueryOptions(client: ApiClient, workspaceSlug: string, query: NotificationListQuery) {
  return infiniteQueryOptions({
    queryKey: notificationKeys.pages(workspaceSlug, query),
    queryFn: ({ pageParam }: { pageParam: string }) =>
      listNotificationsPage(client, workspaceSlug, {
        ...query,
        perPage: NOTIFICATIONS_PAGE_SIZE,
        cursor: pageParam,
      }),
    initialPageParam: NOTIFICATIONS_FIRST_CURSOR,
    getNextPageParam: nextNotificationCursor,
    staleTime: LIST_STALE_TIME_MS,
    enabled: workspaceSlug.length > 0,
    // NTF-028: the cursor stream follows the same pull-only policy as the
    // first-page read.
    refetchOnMount: "always",
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

/** Connected cursor stream used by the inbox list. */
export function useNotificationPages(workspaceSlug: string, query: NotificationListQuery) {
  return useInfiniteQuery(notificationPagesQueryOptions(getAppClient(), workspaceSlug, query));
}
