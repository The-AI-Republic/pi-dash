// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Spreadsheet sub-issue reads (parity/specs/issues.md §3, ISS-015).
// One cached child page per expanded parent; the sheet renders children
// inline to three levels and opens peek on deeper toggles.

import { queryOptions } from "@tanstack/react-query";
import { listSubIssues, type ApiClient } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "../api/keys.js";

export const SHEET_SUB_ISSUE_LIMIT = 100;

export function sheetSubIssuesQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  issueId: string
) {
  return queryOptions({
    queryKey: [...issueKeys.all(workspaceSlug), "sheet-sub-issues", projectId, issueId] as const,
    queryFn: () =>
      listSubIssues(client, workspaceSlug, projectId, issueId, {
        perPage: SHEET_SUB_ISSUE_LIMIT,
        orderBy: "sort_order",
      }),
    staleTime: LIST_STALE_TIME_MS,
  });
}
