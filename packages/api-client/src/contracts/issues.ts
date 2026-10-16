// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue contracts, read-only: flat, grouped and global lists, archived
// list, single-issue detail, picker search, key resolve and description
// versions. The lists answer a slim projection per row; only the detail
// carries the scoring, sync and description fields. Single-issue actions
// (update, delete, cycle/module membership, subscriptions) live in
// issue-actions.ts, out of the initial bundle (NEWFRONT-523, size gate).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDate, nullableDateTime, nullableUuid, uuid } from "./fields.js";

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
  groupBy?: string;
  subGroupBy?: string;
  /** Legacy filter params (state, priority, assignees, ...) passed through verbatim. */
  filters?: Record<string, string | string[]>;
}

function listQueryParams(query: IssueListQuery): Record<string, string | number | boolean | undefined | null> {
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
    query: listQueryParams(query),
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
    query: listQueryParams(query),
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
    query: listQueryParams(query),
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
    query: listQueryParams(query),
  });
  return client.parse(GlobalIssueListResponse, body, "GET global issues");
}

export interface IssueSearchQuery {
  search?: string;
  workspaceSearch?: boolean;
  parent?: boolean;
  issueRelation?: boolean;
  cycle?: boolean;
  module?: string;
  subIssue?: boolean;
  /** `none` keeps only issues with no target date. */
  targetDate?: string;
  issueId?: string;
}

/**
 * One row of the project issue search: the pickers' slim projection
 * (name, key parts, state chip). Doubled keys (`project__name`) are the
 * ORM values() spelling, kept verbatim.
 */
export const IssueSearchRow = z.object({
  name: z.string(),
  id: uuid(),
  start_date: z.nullable(z.iso.date()),
  sequence_id: z.number(),
  project__name: z.string(),
  project__identifier: z.string(),
  project_id: uuid(),
  workspace__slug: z.string(),
  state__name: z.nullable(z.string()),
  state__group: z.nullable(z.string()),
  state__color: z.nullable(z.string()),
});
export type IssueSearchRow = z.infer<typeof IssueSearchRow>;

export const IssueSearchResponse = z.array(IssueSearchRow);
export type IssueSearchResponse = z.infer<typeof IssueSearchResponse>;

/**
 * GET .../search-issues/ — the parent/relation/sub-issue pickers'
 * typeahead. Boolean flags travel as the "true"/"false" strings the
 * view compares against.
 */
export async function searchIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: IssueSearchQuery = {}
): Promise<IssueSearchResponse> {
  const flag = (value: boolean | undefined): string | undefined =>
    value === undefined ? undefined : value ? "true" : "false";
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/search-issues/`, {
    query: {
      search: query.search,
      workspace_search: flag(query.workspaceSearch),
      parent: flag(query.parent),
      issue_relation: flag(query.issueRelation),
      cycle: flag(query.cycle),
      module: query.module,
      sub_issue: flag(query.subIssue),
      target_date: query.targetDate,
      issue_id: query.issueId,
    },
  });
  return client.parse(IssueSearchResponse, body, "GET search issues");
}

/**
 * Lite issue by workspace key (`IDENT-seq`): the browse route's first
 * phase, resolving id and project before the full hydrate. Audit ids
 * are bare user keys here, not the detail's shaped values.
 */
export const LiteIssue = z.object({
  id: uuid(),
  sequence_id: z.number(),
  name: z.string(),
  description_html: z.string(),
  sort_order: z.number(),
  project_id: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  is_draft: z.boolean(),
  is_epic: z.boolean(),
  is_intake: z.boolean(),
  is_synced: z.boolean(),
  archived_at: nullableDate(),
});
export type LiteIssue = z.infer<typeof LiteIssue>;

/** GET /api/workspaces/<slug>/work-items/<IDENT>-<seq>/?lite=1. */
export async function resolveIssueByKey(
  client: ApiClient,
  slug: string,
  projectIdentifier: string,
  sequenceId: number
): Promise<LiteIssue> {
  const path = `/api/workspaces/${slug}/work-items/${projectIdentifier}-${sequenceId}/`;
  const body = await client.getJson<unknown>(path, { query: { lite: "1" } });
  return client.parse(LiteIssue, body, `GET ${path}`);
}

/** One description version row (list view: metadata, no content). */
export const DescriptionVersionRow = z.object({
  id: uuid(),
  workspace: uuid(),
  project: uuid(),
  issue: uuid(),
  last_saved_at: dateTime(),
  owned_by: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
});
export type DescriptionVersionRow = z.infer<typeof DescriptionVersionRow>;

/** Cursor envelope for the description-version history. */
export const DescriptionVersionList = z.object({
  prev_cursor: z.string(),
  cursor: z.string(),
  next_cursor: z.nullable(z.string()),
  prev_page_results: z.boolean(),
  next_page_results: z.boolean(),
  page_count: z.number(),
  total_results: z.number(),
  total_pages: z.number(),
  results: z.array(DescriptionVersionRow),
});
export type DescriptionVersionList = z.infer<typeof DescriptionVersionList>;

/** GET .../work-items/<id>/description-versions/ — version history. */
export async function listDescriptionVersions(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  query: { cursor?: string } = {}
): Promise<DescriptionVersionList> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/work-items/${issueId}/description-versions/`;
  const body = await client.getJson<unknown>(path, { query: { cursor: query.cursor } });
  return client.parse(DescriptionVersionList, body, `GET ${path}`);
}

/** One version with its content: restore reads through this shape. */
export const DescriptionVersion = z.object({
  ...DescriptionVersionRow.shape,
  description_binary: z.nullable(z.string()),
  description_html: z.string(),
  description_stripped: z.nullable(z.string()),
  description_json: z.unknown(),
});
export type DescriptionVersion = z.infer<typeof DescriptionVersion>;

/** GET .../description-versions/<version_id>/ — one version's content. */
export async function getDescriptionVersion(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  versionId: string
): Promise<DescriptionVersion> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/work-items/${issueId}/description-versions/${versionId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(DescriptionVersion, body, `GET ${path}`);
}

export async function getIssue(client: ApiClient, slug: string, projectId: string, issueId: string): Promise<Issue> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/`);
  return client.parse(Issue, body, "GET issue detail");
}

/**
 * Archived issues of one project. Workspace search only indexes live
 * issues, so key resolution falls back to this list for archived keys.
 * Rows share the flat list shape; the envelope carries the same cursor
 * fields as the live list plus grouping extras the schema ignores.
 */
export async function listArchivedIssues(
  client: ApiClient,
  slug: string,
  projectId: string
): Promise<IssueListResponse> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/archived-issues/`);
  return client.parse(IssueListResponse, body, "GET archived issues");
}
