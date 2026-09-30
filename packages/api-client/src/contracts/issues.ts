// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Issue contracts, read-only: ungrouped list and single-issue detail.
// The list answers a slim projection per row; only the detail carries the
// scoring, sync and description fields.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableDateTime, nullableUuid, uuid } from "./fields.js";

export const ISSUE_PRIORITIES = ["urgent", "high", "medium", "low", "none"] as const;

const issueCoreFields = {
  id: uuid(),
  name: z.string(),
  state_id: nullableUuid(),
  sort_order: z.number(),
  completed_at: nullableDateTime(),
  estimate_point: z.optional(z.nullable(z.union([uuid(), z.number()]))),
  priority: z.enum(ISSUE_PRIORITIES),
  start_date: z.optional(z.nullable(z.iso.date())),
  target_date: z.optional(z.nullable(z.iso.date())),
  sequence_id: z.number(),
  project_id: uuid(),
  parent_id: nullableUuid(),
  cycle_id: nullableUuid(),
  module_ids: z.array(uuid()),
  label_ids: z.array(uuid()),
  assignee_ids: z.array(uuid()),
  sub_issues_count: z.nullable(z.number()),
  attachment_count: z.nullable(z.number()),
  link_count: z.nullable(z.number()),
  created_at: z.iso.datetime(),
  updated_at: z.iso.datetime(),
  created_by: z.nullable(z.string()),
  updated_by: z.nullable(z.string()),
  is_draft: z.boolean(),
  archived_at: nullableDateTime(),
};

/**
 * One row of the ungrouped issue list. Count annotations arrive as null
 * when the subquery finds no rows, and audit ids are null for rows the
 * server stamped without a user — neither is normalized away here.
 */
export const IssueListItem = z.object(issueCoreFields);
export type IssueListItem = z.infer<typeof IssueListItem>;

/** Cursor-paginated envelope for the ungrouped issue list. */
export const IssueListResponse = z.object({
  grouped_by: z.nullable(z.string()),
  sub_grouped_by: z.nullable(z.string()),
  total_count: z.number(),
  next_cursor: z.string(),
  prev_cursor: z.string(),
  next_page_results: z.boolean(),
  prev_page_results: z.boolean(),
  count: z.number(),
  total_results: z.number(),
  results: z.array(IssueListItem),
});
export type IssueListResponse = z.infer<typeof IssueListResponse>;

export const IssueRelationsSummary = z.object({
  blocked_by: z.array(z.unknown()),
  blocking: z.array(z.unknown()),
});
export type IssueRelationsSummary = z.infer<typeof IssueRelationsSummary>;

/** Single-issue detail: the list fields plus scoring, sync and content. */
export const Issue = z.object({
  ...issueCoreFields,
  complexity_score: z.number(),
  assigned_pod_id: z.optional(nullableUuid()),
  agent_executor: z.nullable(z.string()),
  is_synced: z.boolean(),
  description_html: z.string(),
  is_subscribed: z.boolean(),
  agent_ticker: z.nullable(z.unknown()),
  agent_status: z.nullable(z.unknown()),
  relations_summary: IssueRelationsSummary,
  has_open_blockers: z.boolean(),
});
export type Issue = z.infer<typeof Issue>;

export interface IssueListQuery {
  perPage?: number;
  cursor?: string;
  orderBy?: string;
}

export async function listIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: IssueListQuery = {}
): Promise<IssueListResponse> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/issues/`, {
    query: {
      per_page: query.perPage,
      cursor: query.cursor,
      order_by: query.orderBy,
    },
  });
  return client.parse(IssueListResponse, body, "GET issues");
}

export async function getIssue(client: ApiClient, slug: string, projectId: string, issueId: string): Promise<Issue> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/`);
  return client.parse(Issue, body, "GET issue detail");
}
