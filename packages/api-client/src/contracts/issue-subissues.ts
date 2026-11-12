// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sub-issue contracts: the paged children read plus the add-existing
// write. Creating a child goes through the shared top-level create
// (NEWFRONT-611 owns that contract); detaching one is a null parent
// write through the shared issue update; deleting one is the shared
// issue delete. This module contracts only the sub-issues collection.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { ISSUE_PRIORITIES, IssueListItem } from "./issues.js";

/**
 * One child row: the flat list projection plus the state group the
 * rollup buckets by. The group is null when the child has no state.
 */
export const SubIssueItem = z.object({
  ...IssueListItem.shape,
  state_group: z.nullable(z.string()),
  /**
   * Work-item type, present only where the edition enables custom types
   * (ISS-234/235). Absence reads as "no type", never as an error.
   */
  type_id: z.optional(nullableUuid()),
});
export type SubIssueItem = z.infer<typeof SubIssueItem>;

/**
 * Children read. Live Django answers the page under `sub_issues` with
 * the full-set `state_distribution` alongside and pages nothing yet;
 * the cursor envelope rides optional until the backend pagination
 * target (main#496) lands. Slices treat absent cursors as exhausted,
 * which is also the small-parent shape after paging lands.
 */
export const SubIssuesResponse = z.object({
  sub_issues: z.array(SubIssueItem),
  state_distribution: z.record(z.string(), z.array(z.string())),
  total_count: z.optional(z.number()),
  next_cursor: z.optional(z.string()),
  prev_cursor: z.optional(z.string()),
  next_page_results: z.optional(z.boolean()),
  prev_page_results: z.optional(z.boolean()),
  count: z.optional(z.number()),
  total_pages: z.optional(z.number()),
});
export type SubIssuesResponse = z.infer<typeof SubIssuesResponse>;

export interface SubIssuesQuery {
  perPage?: number;
  cursor?: string;
  orderBy?: string;
}

export async function listSubIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  query: SubIssuesQuery = {}
): Promise<SubIssuesResponse> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/sub-issues/`;
  const body = await client.getJson<unknown>(path, {
    query: {
      per_page: query.perPage,
      cursor: query.cursor,
      order_by: query.orderBy,
    },
  });
  return client.parse(SubIssuesResponse, body, `GET ${path}`);
}

/**
 * One row of the add-existing answer: the detail serializer shape the
 * collection POST returns for every attached child. The membership
 * and count annotations the list carries are absent here — the POST
 * annotates nothing — so the row is exactly these fields.
 */
export const AddedSubIssue = z.object({
  id: uuid(),
  name: z.string(),
  state_id: nullableUuid(),
  sort_order: z.number(),
  completed_at: nullableDateTime(),
  estimate_point: z.nullable(z.union([uuid(), z.number()])),
  priority: z.enum(ISSUE_PRIORITIES),
  complexity_score: z.number(),
  start_date: z.nullable(z.iso.date()),
  target_date: z.nullable(z.iso.date()),
  sequence_id: z.number(),
  project_id: uuid(),
  parent_id: nullableUuid(),
  assigned_pod_id: nullableUuid(),
  agent_executor: z.nullable(z.string()),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: z.nullable(z.string()),
  updated_by: z.nullable(z.string()),
  is_draft: z.boolean(),
  archived_at: z.nullable(z.iso.date()),
  is_synced: z.boolean(),
});
export type AddedSubIssue = z.infer<typeof AddedSubIssue>;

export const AddSubIssuesResponse = z.object({
  sub_issues: z.array(AddedSubIssue),
  state_distribution: z.record(z.string(), z.array(z.string())),
});
export type AddSubIssuesResponse = z.infer<typeof AddSubIssuesResponse>;

/**
 * Attach existing issues as children of the parent. Answers the
 * attached rows plus the refreshed distribution. An empty id list is
 * a 400; unknown ids are silently skipped.
 */
export async function addSubIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  subIssueIds: string[]
): Promise<AddSubIssuesResponse> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/sub-issues/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { sub_issue_ids: subIssueIds } });
  return client.parse(AddSubIssuesResponse, envelope.data, `POST ${path}`);
}
