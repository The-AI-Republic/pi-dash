// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Notification queries. The route loader prefetches the unread count
// for the top-bar inbox dot (SHELL-065); the query stays disabled
// without a workspace so no request fires.
import { queryOptions } from "@tanstack/react-query";
import { getUnreadNotifications, type ApiClient } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { notificationKeys } from "./keys.js";

export function unreadNotificationsQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: notificationKeys.unread(workspaceSlug),
    queryFn: () => getUnreadNotifications(client, workspaceSlug),
    staleTime: LIST_STALE_TIME_MS,
    enabled: workspaceSlug.length > 0,
  });
}
