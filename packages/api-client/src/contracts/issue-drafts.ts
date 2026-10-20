// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Draft-issue contracts: the workspace drafts read plus the create,
// update, delete and promote-to-project writes. The list only ever
// carries the caller's own drafts, newest first.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { ISSUE_PRIORITIES } from "./issues.js";

/** One draft row: the issue projection minus the live annotations. */
export const DraftIssue = z.object({
  id: uuid(),
  name: z.nullable(z.string()),
  state_id: nullableUuid(),
  sort_order: z.number(),
  completed_at: nullableDateTime(),
  estimate_point: z.optional(z.nullable(z.union([uuid(), z.number()]))),
  priority: z.enum(ISSUE_PRIORITIES),
  start_date: z.optional(z.nullable(z.iso.date())),
  target_date: z.optional(z.nullable(z.iso.date())),
  project_id: nullableUuid(),
  parent_id: nullableUuid(),
  cycle_id: nullableUuid(),
  module_ids: z.array(uuid()),
  label_ids: z.array(uuid()),
  assignee_ids: z.array(uuid()),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  type_id: nullableUuid(),
  description_html: z.string(),
});
export type DraftIssue = z.infer<typeof DraftIssue>;

/** Cursor-paginated envelope for the workspace drafts list. */
export const DraftIssueListResponse = z.object({
  grouped_by: z.nullable(z.string()),
  sub_grouped_by: z.nullable(z.string()),
  total_count: z.number(),
  next_cursor: z.string(),
  prev_cursor: z.string(),
  next_page_results: z.boolean(),
  prev_page_results: z.boolean(),
  count: z.number(),
  total_results: z.number(),
  results: z.array(DraftIssue),
});
export type DraftIssueListResponse = z.infer<typeof DraftIssueListResponse>;

export interface DraftIssueListQuery {
  perPage?: number;
  cursor?: string;
}

export async function listDraftIssues(
  client: ApiClient,
  slug: string,
  query: DraftIssueListQuery = {}
): Promise<DraftIssueListResponse> {
  const path = `/api/workspaces/${slug}/draft-issues/`;
  const body = await client.getJson<unknown>(path, {
    query: { per_page: query.perPage, cursor: query.cursor },
  });
  return client.parse(DraftIssueListResponse, body, `GET ${path}`);
}

export async function getDraftIssue(client: ApiClient, slug: string, draftId: string): Promise<DraftIssue> {
  const path = `/api/workspaces/${slug}/draft-issues/${draftId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(DraftIssue, body, `GET ${path}`);
}

export interface DraftIssueCreateInput {
  name?: string | null;
  project_id?: string | null;
  description_html?: string;
  state_id?: string | null;
  priority?: (typeof ISSUE_PRIORITIES)[number];
  parent_id?: string | null;
  assignee_ids?: string[];
  label_ids?: string[];
  start_date?: string | null;
  target_date?: string | null;
  /** Estimate point id; null clears the estimate. */
  estimate_point?: string | null;
  /** Cycle the draft is filed under; null clears it. */
  cycle_id?: string | null;
  /**
   * The draft's complete module set. The update serializer replaces the
   * stored set rather than merging, so callers send every kept id.
   */
  module_ids?: string[];
}

/**
 * Save a draft. Answers the stored row. Only the caller may later
 * read, edit or delete it.
 */
export async function createDraftIssue(
  client: ApiClient,
  slug: string,
  input: DraftIssueCreateInput
): Promise<DraftIssue> {
  const path = `/api/workspaces/${slug}/draft-issues/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(DraftIssue, envelope.data, `POST ${path}`);
}

/**
 * Patch a draft. Answers 204 with no body, so callers re-read through
 * getDraftIssue. Unknown ids answer 404.
 */
export async function updateDraftIssue(
  client: ApiClient,
  slug: string,
  draftId: string,
  input: DraftIssueCreateInput
): Promise<void> {
  await client.patch(`/api/workspaces/${slug}/draft-issues/${draftId}/`, { json: input });
}

export async function deleteDraftIssue(client: ApiClient, slug: string, draftId: string): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/draft-issues/${draftId}/`);
}

/**
 * The issue a promote answers: the raw create-serializer shape, which
 * carries the stored row without the list's membership and count
 * annotations.
 */
export const PromotedIssue = z.object({
  id: uuid(),
  name: z.string(),
  project_id: uuid(),
  sequence_id: z.number(),
  state_id: nullableUuid(),
  priority: z.enum(ISSUE_PRIORITIES),
  sort_order: z.number(),
  parent_id: nullableUuid(),
  description_html: z.string(),
  created_at: dateTime(),
  updated_at: dateTime(),
  deleted_at: nullableDateTime(),
});
export type PromotedIssue = z.infer<typeof PromotedIssue>;

/**
 * Promote a draft into a project issue. The body carries the issue
 * fields (the draft's own payload); the draft must name a project or
 * the server answers 400. Answers the created issue and deletes the
 * draft.
 */
export async function promoteDraftIssue(
  client: ApiClient,
  slug: string,
  draftId: string,
  input: DraftIssueCreateInput
): Promise<PromotedIssue> {
  const path = `/api/workspaces/${slug}/draft-to-issue/${draftId}/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(PromotedIssue, envelope.data, `POST ${path}`);
}
