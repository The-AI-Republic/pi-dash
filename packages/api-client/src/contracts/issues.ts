// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue contracts, read-only: flat, grouped and global lists, archived
// list and single-issue detail. Picker search and description versions
// live with NEWFRONT-612, key resolve with NEWFRONT-613. The lists
// answer a slim projection per row; only the detail carries the scoring,
// sync and description fields. Single-issue actions (update, delete,
// cycle/module membership, subscriptions) live in issue-actions.ts, out
// of the initial bundle (NEWFRONT-523, size gate).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDate, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { AgentStatus, AgentTicker } from "./issue-agent.js";

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
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: z.nullable(z.string()),
  updated_by: z.nullable(z.string()),
  is_draft: z.boolean(),
  // Epic marker for the project tab highlight (SHELL-076): only some
  // serializers send it, so readers treat absence as a plain work item.
  is_epic: z.optional(z.boolean()),
  // The model stamps a calendar date (DateField), not a datetime.
  archived_at: nullableDate(),
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
  agent_ticker: z.nullable(AgentTicker),
  agent_status: z.nullable(AgentStatus),
  relations_summary: IssueRelationsSummary,
  has_open_blockers: z.boolean(),
});
export type Issue = z.infer<typeof Issue>;

export interface IssueListQuery {
  perPage?: number;
  cursor?: string;
  orderBy?: string;
  groupBy?: string;
  subGroupBy?: string;
  /** Legacy filter params (state, priority, assignees, ...) passed through verbatim. */
  filters?: Record<string, string | string[]>;
}

export function issueListQueryParams(
  query: IssueListQuery
): Record<string, string | number | boolean | undefined | null> {
  const params: Record<string, string | number | boolean | undefined | null> = {
    per_page: query.perPage,
    cursor: query.cursor,
    order_by: query.orderBy,
    group_by: query.groupBy,
    sub_group_by: query.subGroupBy,
  };
  for (const [key, value] of Object.entries(query.filters ?? {})) {
    params[key] = Array.isArray(value) ? value.join(",") : value;
  }
  return params;
}

export async function listIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: IssueListQuery = {}
): Promise<IssueListResponse> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/issues/`, {
    query: issueListQueryParams(query),
  });
  return client.parse(IssueListResponse, body, "GET issues");
}

/**
 * One row of a grouped issue list. Grouped reads add the `state__group`
 * annotation, and grouping by a to-many dimension swaps the aggregated
 * array for the raw join key (`labels__id`, `assignees__id`,
 * `issue_module__module_id`), so the arrays are optional here.
 */
export const GroupedIssueRow = z.object({
  ...issueCoreFields,
  module_ids: z.optional(z.array(uuid())),
  label_ids: z.optional(z.array(uuid())),
  assignee_ids: z.optional(z.array(uuid())),
  state__group: z.optional(z.nullable(z.string())),
  labels__id: z.optional(z.nullable(uuid())),
  assignees__id: z.optional(z.nullable(uuid())),
  issue_module__module_id: z.optional(z.nullable(uuid())),
});
export type GroupedIssueRow = z.infer<typeof GroupedIssueRow>;

/** One group bucket: the page's rows plus the group's total. */
export const IssueGroupBucket = z.object({
  results: z.array(GroupedIssueRow),
  total_results: z.number(),
});
export type IssueGroupBucket = z.infer<typeof IssueGroupBucket>;

/** Grouped list envelope: `results` maps group value to its bucket. */
export const GroupedIssueListResponse = z.object({
  grouped_by: z.nullable(z.string()),
  sub_grouped_by: z.nullable(z.string()),
  total_count: z.number(),
  next_cursor: z.string(),
  prev_cursor: z.string(),
  next_page_results: z.boolean(),
  prev_page_results: z.boolean(),
  count: z.number(),
  total_results: z.number(),
  results: z.record(z.string(), IssueGroupBucket),
});
export type GroupedIssueListResponse = z.infer<typeof GroupedIssueListResponse>;

/** Sub-grouped envelope: each group bucket holds sub-group buckets. */
export const SubGroupedIssueBucket = z.object({
  results: z.record(z.string(), IssueGroupBucket),
  total_results: z.number(),
});
export type SubGroupedIssueBucket = z.infer<typeof SubGroupedIssueBucket>;

export const SubGroupedIssueListResponse = z.object({
  grouped_by: z.nullable(z.string()),
  sub_grouped_by: z.nullable(z.string()),
  total_count: z.number(),
  next_cursor: z.string(),
  prev_cursor: z.string(),
  next_page_results: z.boolean(),
  prev_page_results: z.boolean(),
  count: z.number(),
  total_results: z.number(),
  results: z.record(z.string(), SubGroupedIssueBucket),
});
export type SubGroupedIssueListResponse = z.infer<typeof SubGroupedIssueListResponse>;

/** GET .../issues/ with group_by: one bucket per group value. */
export async function listGroupedIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: IssueListQuery & { groupBy: string }
): Promise<GroupedIssueListResponse> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/issues/`, {
    query: issueListQueryParams(query),
  });
  return client.parse(GroupedIssueListResponse, body, "GET grouped issues");
}

/** GET .../issues/ with group_by + sub_group_by: buckets within buckets. */
export async function listSubGroupedIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: IssueListQuery & { groupBy: string; subGroupBy: string }
): Promise<SubGroupedIssueListResponse> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/issues/`, {
    query: issueListQueryParams(query),
  });
  return client.parse(SubGroupedIssueListResponse, body, "GET sub-grouped issues");
}

/**
 * One row of the workspace all-issues list: the flat list shape plus
 * the `state__group` annotation the global serializer always sends.
 */
export const GlobalIssueRow = z.object({
  ...issueCoreFields,
  state__group: z.nullable(z.string()),
});
export type GlobalIssueRow = z.infer<typeof GlobalIssueRow>;

/** Cursor-paginated envelope for the workspace all-issues list. */
export const GlobalIssueListResponse = z.object({
  grouped_by: z.nullable(z.string()),
  sub_grouped_by: z.nullable(z.string()),
  total_count: z.number(),
  next_cursor: z.string(),
  prev_cursor: z.string(),
  next_page_results: z.boolean(),
  prev_page_results: z.boolean(),
  count: z.number(),
  total_results: z.number(),
  results: z.array(GlobalIssueRow),
});
export type GlobalIssueListResponse = z.infer<typeof GlobalIssueListResponse>;

/** GET /api/workspaces/<slug>/issues/ — every visible issue, all projects. */
export async function listGlobalIssues(
  client: ApiClient,
  slug: string,
  query: IssueListQuery = {}
): Promise<GlobalIssueListResponse> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/issues/`, {
    query: issueListQueryParams(query),
  });
  return client.parse(GlobalIssueListResponse, body, "GET global issues");
}

export async function getIssue(client: ApiClient, slug: string, projectId: string, issueId: string): Promise<Issue> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/`);
  return client.parse(Issue, body, "GET issue detail");
}

/**
 * Archived issues of one project. Workspace search only indexes live
 * issues, so key resolution falls back to this list for archived keys.
 * Rows share the flat list shape; the envelope carries the same cursor
 * fields as the live list plus grouping extras the schema ignores. The
 * archived tab sends the live list query (order_by, group_by, the
 * JSON-stringified rich expression in `filters`) through the same
 * builder, so this reader takes the same IssueListQuery.
 */
export async function listArchivedIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: IssueListQuery = {}
): Promise<IssueListResponse> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/archived-issues/`, {
    query: issueListQueryParams(query),
  });
  return client.parse(IssueListResponse, body, "GET archived issues");
}

/** GET .../archived-issues/ with group_by: one bucket per group value. */
export async function listGroupedArchivedIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: IssueListQuery & { groupBy: string }
): Promise<GroupedIssueListResponse> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/archived-issues/`, {
    query: issueListQueryParams(query),
  });
  return client.parse(GroupedIssueListResponse, body, "GET grouped archived issues");
}

/** GET .../archived-issues/ with group_by + sub_group_by: buckets within buckets. */
export async function listSubGroupedArchivedIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: IssueListQuery & { groupBy: string; subGroupBy: string }
): Promise<SubGroupedIssueListResponse> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/archived-issues/`, {
    query: issueListQueryParams(query),
  });
  return client.parse(SubGroupedIssueListResponse, body, "GET sub-grouped archived issues");
}

/**
 * One archived issue, read through its archive-scoped path. The backend
 * serves the same detail serializer as the live read but without the
 * annotation-backed keys — cycle, module/label/assignee memberships and
 * the sub-issue, attachment and link counts are always absent here, so
 * this schema is deliberately narrower than Issue. The archived tab and
 * detail must not depend on live rows, so the app calls this — not the
 * live detail — for archived ids.
 */
export const ArchivedIssue = z.object({
  id: uuid(),
  name: z.string(),
  state_id: nullableUuid(),
  sort_order: z.number(),
  completed_at: nullableDateTime(),
  estimate_point: z.optional(z.nullable(z.union([uuid(), z.number()]))),
  priority: z.enum(ISSUE_PRIORITIES),
  complexity_score: z.number(),
  start_date: z.optional(z.nullable(z.iso.date())),
  target_date: z.optional(z.nullable(z.iso.date())),
  sequence_id: z.number(),
  project_id: uuid(),
  parent_id: nullableUuid(),
  assigned_pod_id: z.optional(nullableUuid()),
  agent_executor: z.nullable(z.string()),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: z.nullable(z.string()),
  updated_by: z.nullable(z.string()),
  is_draft: z.boolean(),
  archived_at: nullableDate(),
  is_synced: z.boolean(),
  description_html: z.string(),
  is_subscribed: z.boolean(),
  agent_ticker: z.nullable(AgentTicker),
  agent_status: z.nullable(AgentStatus),
  relations_summary: IssueRelationsSummary,
  has_open_blockers: z.boolean(),
});
export type ArchivedIssue = z.infer<typeof ArchivedIssue>;

export async function getArchivedIssue(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string
): Promise<ArchivedIssue> {
  const body = await client.getJson<unknown>(
    `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/archive/`
  );
  return client.parse(ArchivedIssue, body, "GET archived issue detail");
}

/**
 * One cycle's issues (the cycle page's list layout). Same envelope and
 * row projection as the project list; the path scopes to the cycle.
 */
export async function listCycleIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string,
  query: IssueListQuery = {}
): Promise<IssueListResponse> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/cycle-issues/`;
  const body = await client.getJson<unknown>(path, { query: issueListQueryParams(query) });
  return client.parse(IssueListResponse, body, `GET ${path}`);
}

/** Grouped variant of the cycle list; same bucket shape as the project. */
export async function listGroupedCycleIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string,
  query: IssueListQuery & { groupBy: string }
): Promise<GroupedIssueListResponse> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/cycle-issues/`;
  const body = await client.getJson<unknown>(path, { query: issueListQueryParams(query) });
  return client.parse(GroupedIssueListResponse, body, `GET ${path}`);
}

/** One module's issues (the module page's list layout). */
export async function listModuleIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string,
  query: IssueListQuery = {}
): Promise<IssueListResponse> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/modules/${moduleId}/issues/`;
  const body = await client.getJson<unknown>(path, { query: issueListQueryParams(query) });
  return client.parse(IssueListResponse, body, `GET ${path}`);
}

/** Grouped variant of the module list; same bucket shape as the project. */
export async function listGroupedModuleIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string,
  query: IssueListQuery & { groupBy: string }
): Promise<GroupedIssueListResponse> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/modules/${moduleId}/issues/`;
  const body = await client.getJson<unknown>(path, { query: issueListQueryParams(query) });
  return client.parse(GroupedIssueListResponse, body, `GET ${path}`);
}
