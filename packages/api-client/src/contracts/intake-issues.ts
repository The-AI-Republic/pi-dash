// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake-issues contracts: the intake list, the request detail, create,
// triage/edit and delete. The list/detail routes exist at both
// `intake-issues/` and the legacy `inbox-issues/` alias (same viewset);
// the new build calls `intake-issues/` only. The `<uuid:pk>` on the
// detail routes is the work-item id, not the intake row's own id. The
// intake-container endpoints (`intakes/`, `inboxes/`) are rowless by
// inventory design and stay uncontracted. Description versions ride the
// existing `work-items/` contract in issue-versions.ts (INT-029); there
// is deliberately no second contract for the `intake-work-items/` alias.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { ISSUE_PRIORITIES, Issue } from "./issues.js";

/**
 * Intake request statuses (`IntakeIssueStatus`). The model names -1
 * REJECTED; the screen calls it Declined, which is the name used here.
 * Pending (-2) is also the server's default list filter.
 */
export const INTAKE_STATUS = {
  pending: -2,
  declined: -1,
  snoozed: 0,
  accepted: 1,
  duplicate: 2,
} as const;
export type IntakeStatus = (typeof INTAKE_STATUS)[keyof typeof INTAKE_STATUS];

/**
 * The list row's embedded work item (`IssueIntakeSerializer`): the slim
 * projection the list needs — identity, title, priority, sequence and
 * timestamps — plus `label_ids` for the row's label chips. The
 * annotation behind `label_ids` is only attached on the list and detail
 * paths, so the key is optional: the duplicate-target summary carried on
 * the detail (`duplicate_issue_detail`) omits it. `created_by` is the
 * work item's creator (the row's author avatar); it is null for rows the
 * server stamped without a user.
 */
export const IntakeWorkItemSummary = z.object({
  id: uuid(),
  name: z.string(),
  priority: z.enum(ISSUE_PRIORITIES),
  sequence_id: z.number(),
  project_id: uuid(),
  created_at: dateTime(),
  label_ids: z.optional(z.array(uuid())),
  created_by: z.nullable(z.string()),
});
export type IntakeWorkItemSummary = z.infer<typeof IntakeWorkItemSummary>;

/**
 * One intake list row (`IntakeIssueSerializer`): the intake entry's own
 * id, status, duplicate target, snooze and source, the embedded work
 * item, and `created_by` — the intake entry's creator, who can differ
 * from the work item's creator and can be null for rows seeded or
 * ingested outside a request. The row carries no project or workspace
 * keys; callers already know the scope from the path.
 */
export const IntakeIssueRow = z.object({
  id: uuid(),
  status: z.number(),
  duplicate_to: nullableUuid(),
  snoozed_till: nullableDateTime(),
  source: z.nullable(z.string()),
  issue: IntakeWorkItemSummary,
  created_by: z.nullable(z.string()),
});
export type IntakeIssueRow = z.infer<typeof IntakeIssueRow>;

/**
 * Cursor-paginated envelope for the intake list: the same BasePaginator
 * keys as the work-item list (`next_cursor` drives the next page, and
 * `next_page_results` says whether one exists). `total_pages` and
 * `extra_stats` ride along but no screen reads them, so the schema
 * ignores them.
 */
export const IntakeIssueListResponse = z.object({
  grouped_by: z.nullable(z.string()),
  sub_grouped_by: z.nullable(z.string()),
  total_count: z.number(),
  next_cursor: z.string(),
  prev_cursor: z.string(),
  next_page_results: z.boolean(),
  prev_page_results: z.boolean(),
  count: z.number(),
  total_results: z.number(),
  results: z.array(IntakeIssueRow),
});
export type IntakeIssueListResponse = z.infer<typeof IntakeIssueListResponse>;

export interface IntakeIssueListQuery {
  /** Intake statuses to include; the screen always sends the tab's set. */
  statuses?: IntakeStatus[];
  /** Priorities to include; the server unions them. */
  priorities?: string[];
  /** Assignee user ids; non-UUID values are dropped server-side. */
  assignees?: string[];
  /**
   * Creator user ids. Despite the name this filters the work item's
   * creator (`issue__created_by`), not the intake entry's audit user.
   */
  createdBy?: string[];
  /** Label ids; non-UUID values are dropped server-side. */
  labels?: string[];
  /**
   * Created-day filter, one encoded range verbatim (for example
   * `2026-10-01;after,2026-10-31;before`); the client builds the token,
   * the contract only carries it.
   */
  createdAt?: string;
  /** Updated-day filter, same encoding (backend defect NEWFRONT-270: the server predicates the created column). */
  updatedAt?: string;
  /** `issue__created_at`, `issue__updated_at` or `issue__sequence_id`, `-`-prefixed for descending. */
  orderBy?: string;
  cursor?: string;
  perPage?: number;
}

export function intakeIssueListQueryParams(
  query: IntakeIssueListQuery
): Record<string, string | number | boolean | undefined | null> {
  return {
    status: query.statuses === undefined ? undefined : query.statuses.join(","),
    priority: query.priorities === undefined ? undefined : query.priorities.join(","),
    assignees: query.assignees === undefined ? undefined : query.assignees.join(","),
    created_by: query.createdBy === undefined ? undefined : query.createdBy.join(","),
    labels: query.labels === undefined ? undefined : query.labels.join(","),
    created_at: query.createdAt,
    updated_at: query.updatedAt,
    order_by: query.orderBy,
    cursor: query.cursor,
    per_page: query.perPage,
  };
}

export async function listIntakeIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: IntakeIssueListQuery = {}
): Promise<IntakeIssueListResponse> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/intake-issues/`;
  const body = await client.getJson<unknown>(path, { query: intakeIssueListQueryParams(query) });
  return client.parse(IntakeIssueListResponse, body, `GET ${path}`);
}

/**
 * The detail's embedded work item: the full work-item detail shape, but
 * the intake viewset never annotates the count/cycle/module/subscription
 * keys the work-item viewset adds, and DRF drops read-only fields whose
 * attributes are missing — so `cycle_id`, `module_ids`,
 * `sub_issues_count`, `attachment_count`, `link_count`,
 * `is_subscribed` and `is_intake` are absent here, not null. The detail
 * serializer does annotate `label_ids` and `assignee_ids`, so those stay
 * required.
 */
export const IntakeIssueDetailItem = z.object({
  ...Issue.shape,
  cycle_id: z.optional(nullableUuid()),
  module_ids: z.optional(z.array(uuid())),
  sub_issues_count: z.optional(z.nullable(z.number())),
  attachment_count: z.optional(z.nullable(z.number())),
  link_count: z.optional(z.nullable(z.number())),
  is_subscribed: z.optional(z.boolean()),
  is_intake: z.optional(z.boolean()),
});
export type IntakeIssueDetailItem = z.infer<typeof IntakeIssueDetailItem>;

/**
 * Request detail (`IntakeIssueDetailSerializer`): the intake entry's own
 * fields plus the full work item. `duplicate_issue_detail` carries the
 * duplicate target's summary (without `label_ids`) when the request was
 * marked duplicate, null otherwise. Unlike the list row there is no
 * `created_by` here; the author reads from the embedded work item.
 */
export const IntakeIssueDetail = z.object({
  id: uuid(),
  status: z.number(),
  duplicate_to: nullableUuid(),
  snoozed_till: nullableDateTime(),
  duplicate_issue_detail: z.nullable(IntakeWorkItemSummary),
  source: z.nullable(z.string()),
  issue: IntakeIssueDetailItem,
});
export type IntakeIssueDetail = z.infer<typeof IntakeIssueDetail>;

/** GET one request; `issueId` is the work-item id. Unknown ids 404, which the screen turns into a redirect. */
export async function getIntakeIssue(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string
): Promise<IntakeIssueDetail> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/intake-issues/${issueId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(IntakeIssueDetail, body, `GET ${path}`);
}

export interface IntakeIssueCreateIssue {
  /** Required, max 255; empty names are refused with a 400, not client-blocked. */
  name: string;
  description_html?: string;
  priority?: string;
  /** Carried for clarity; the server overwrites it with the triage state. */
  state_id?: string;
  label_ids?: string[];
  assignee_ids?: string[];
  /** Due date, ISO day. */
  target_date?: string;
}

export interface IntakeIssueCreateInput {
  /** Sent as `IN_APP` for clarity; the server writes its own constant. */
  source?: string;
  issue: IntakeIssueCreateIssue;
}

/**
 * Create a request. Answers 200 (not 201) with the created request's
 * detail payload. The server forces the project's triage state
 * (creating it when the project has none), tags the source as in-app,
 * and logs a creation activity on the work item. Missing names and
 * priorities outside the five-value set are 400s (`{error}`).
 */
export async function createIntakeIssue(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: IntakeIssueCreateInput
): Promise<IntakeIssueDetail> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/intake-issues/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(IntakeIssueDetail, envelope.data, `POST ${path}`);
}

export interface IntakeIssueUpdateIssue {
  name?: string;
  description_html?: string;
  priority?: string;
  assignee_ids?: string[];
  label_ids?: string[];
  target_date?: string | null;
  state_id?: string;
}

export interface IntakeIssueUpdateInput {
  /** Status move (accept/decline/snooze/duplicate); admin-only, see below. */
  status?: IntakeStatus;
  /** Snooze day as an ISO timestamp; null clears it on un-snooze. */
  snoozed_till?: string | null;
  /**
   * Duplicate target work-item id. It must be a regular work item: the
   * validator's issue queryset excludes triage-state rows, so another
   * pending request's id is refused as unknown.
   */
  duplicate_to?: string | null;
  /** Inline work-item edits, nested under `issue`. */
  issue?: IntakeIssueUpdateIssue;
}

/**
 * Triage and inline edits. Answers the updated detail payload.
 *
 * Role limits the server enforces: the endpoint is admin-only with a
 * creator bypass, so a member or guest editing someone else's request
 * is refused with 403 before any write runs. A creator's status move
 * without admin rights is silently ignored with a 200 (no intake
 * serializer runs for them); a guest creator's work-item write is
 * narrowed to name and description, other fields ignored with a 200.
 * Accepting needs a project default state, else a 400; the accept moves
 * the work item from triage to that default state.
 */
export async function updateIntakeIssue(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  input: IntakeIssueUpdateInput
): Promise<IntakeIssueDetail> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/intake-issues/${issueId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(IntakeIssueDetail, envelope.data, `PATCH ${path}`);
}

/**
 * Delete a request; answers 204 with no body, so this resolves the
 * response status. For any status other than accepted the server removes
 * the work item too; an accepted request's work item survives.
 * Admin-or-creator only (a member deleting someone else's request gets
 * 403); the UI offers the control on pending and snoozed requests.
 */
export async function deleteIntakeIssue(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string
): Promise<number> {
  const response = await client.delete(`/api/workspaces/${slug}/projects/${projectId}/intake-issues/${issueId}/`);
  return response.status;
}
