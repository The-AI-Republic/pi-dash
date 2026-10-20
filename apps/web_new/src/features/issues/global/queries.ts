// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace all-issues read (parity/specs/issues.md §18, ISS-223). The
// route filters stay out of the query: this build renders the same
// unfiltered list with or without them, which the oracle pins. The
// identity still keys on nothing but the workspace so the plain view
// and a filtered deep link share one cache entry.

import { queryOptions } from "@tanstack/react-query";
import { listGlobalIssues, type ApiClient } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "../api/keys.js";

/** Rows per request for the workspace all-issues view. */
export const GLOBAL_ISSUES_PAGE_SIZE = 100;

/** The workspace-wide issue list, every joined project included. */
export function globalIssuesQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: [...issueKeys.all(workspaceSlug), "global"] as const,
    queryFn: () => listGlobalIssues(client, workspaceSlug, { perPage: GLOBAL_ISSUES_PAGE_SIZE }),
    staleTime: LIST_STALE_TIME_MS,
  });
}
