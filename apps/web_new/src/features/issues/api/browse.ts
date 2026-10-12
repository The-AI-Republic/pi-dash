// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Browse resolution (SHELL-106, from parity/specs/shell.md §1). The
// workspace-level browse route renders the project-scoped detail view,
// never a workspace-wide browser: the key in the route resolves to the
// internal id through workspace search, then detail loads through the
// issues area's read path. Full detail ownership stays with NEWFRONT-71.

import { queryOptions } from "@tanstack/react-query";
import { getIssue, searchWorkspace, type ApiClient, type Issue, type SearchResults } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "./keys.js";

export type BrowseSearchRow = NonNullable<NonNullable<SearchResults["results"]>["issue"]>[number];

export interface BrowseResolution {
  issue: Issue | null;
  projectId: string | null;
}

/**
 * Exact key match on the identifier-plus-sequence pair (for example
 * ACME-42). Prefix orPartial matches stay out: the route names one item.
 */
export function matchBrowseRow(rows: BrowseSearchRow[], issueKey: string): BrowseSearchRow | null {
  if (issueKey.length === 0) return null;
  for (const row of rows) {
    if (`${row.project__identifier}-${row.sequence_id}` === issueKey) return row;
  }
  return null;
}

/** Resolve a browse key to the project-scoped detail (null when unknown). */
export async function loadBrowseIssue(
  client: ApiClient,
  workspaceSlug: string,
  issueKey: string
): Promise<BrowseResolution> {
  const found = await searchWorkspace(client, workspaceSlug, {
    search: issueKey,
    workspaceSearch: true,
    entities: ["issue"],
  });
  const row = matchBrowseRow(found.results.issue ?? [], issueKey);
  if (row === null) return { issue: null, projectId: null };
  const issue = await getIssue(client, workspaceSlug, row.project_id, row.id);
  return { issue, projectId: row.project_id };
}

export function browseIssueQueryOptions(client: ApiClient, workspaceSlug: string, issueKey: string) {
  return queryOptions({
    queryKey: issueKeys.browse(workspaceSlug, issueKey),
    queryFn: () => loadBrowseIssue(client, workspaceSlug, issueKey),
    staleTime: LIST_STALE_TIME_MS,
  });
}
