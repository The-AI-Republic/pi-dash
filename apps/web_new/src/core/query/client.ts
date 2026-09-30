// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Shared TanStack Query client (Data layer page). Feature modules own
// their key factories and queryOptions; every key starts with the
// workspace scope below so a workspace switch or sign-out clears one
// subtree.

import { QueryClient } from "@tanstack/react-query";
import { isRetryable } from "@pidash/api-client";

/** Default freshness for lists. Reference data uses REFERENCE_STALE_TIME_MS. */
export const LIST_STALE_TIME_MS = 30_000;

/** Freshness for slowly-changing data: states, labels, members, projects. */
export const REFERENCE_STALE_TIME_MS = 5 * 60_000;

/** Root of every feature key: ["ws", workspaceSlug, ...rest]. */
export function workspaceScope(workspaceSlug: string): readonly ["ws", string] {
  return ["ws", workspaceSlug] as const;
}

/** One client per app bootstrap. Features receive it through context. */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: LIST_STALE_TIME_MS,
        refetchOnWindowFocus: true,
        retry: (failureCount, error) => failureCount < 2 && isRetryable(error),
      },
      mutations: {
        retry: false,
      },
    },
  });
}
