// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The first page of one work item's children, for the list row expander
// (parity/specs/issues.md §8, ISS-082). Only the lazy list layout asks for
// it, so the read sits apart from the list/detail options the first route
// needs and its api-client sub-issue contract rides the layout's chunk
// (NEWFRONT-871, size gate).

import { queryOptions } from "@tanstack/react-query";
import { listSubIssues, type ApiClient } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "./keys.js";

/** Children of one work item, loaded lazily by the row expander. */
export function subIssuesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, issueId: string) {
  return queryOptions({
    queryKey: [...issueKeys.all(workspaceSlug), "sub-issues", projectId, issueId] as const,
    queryFn: () => listSubIssues(client, workspaceSlug, projectId, issueId, { perPage: 50 }),
    staleTime: LIST_STALE_TIME_MS,
  });
}
