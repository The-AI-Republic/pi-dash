// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue-relation contracts: the grouped read plus the add and remove
// writes. The server stores one directed row per edge; the list
// answers both directions by querying each side, so adding A-to-B
// also shows B-to-A until the edge is removed.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableUuid, uuid } from "./fields.js";
import { ISSUE_PRIORITIES } from "./issues.js";

/** The eight groups the relations read answers, keyed by direction. */
export const ISSUE_RELATION_TYPES = [
  "blocking",
  "blocked_by",
  "duplicate",
  "relates_to",
  "start_after",
  "start_before",
  "finish_after",
  "finish_before",
] as const;
export type IssueRelationType = (typeof ISSUE_RELATION_TYPES)[number];

/** One related row: the slim projection the grouped read carries. */
export const RelatedIssue = z.object({
  id: uuid(),
  name: z.string(),
  state_id: nullableUuid(),
  sort_order: z.number(),
  priority: z.enum(ISSUE_PRIORITIES),
  sequence_id: z.number(),
  project_id: uuid(),
  label_ids: z.array(uuid()),
  assignee_ids: z.array(uuid()),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  relation_type: z.enum(ISSUE_RELATION_TYPES),
});
export type RelatedIssue = z.infer<typeof RelatedIssue>;

/** The relations read: all eight groups, empty arrays when absent. */
export const IssueRelations = z.object({
  blocking: z.array(RelatedIssue),
  blocked_by: z.array(RelatedIssue),
  duplicate: z.array(RelatedIssue),
  relates_to: z.array(RelatedIssue),
  start_after: z.array(RelatedIssue),
  start_before: z.array(RelatedIssue),
  finish_after: z.array(RelatedIssue),
  finish_before: z.array(RelatedIssue),
});
export type IssueRelations = z.infer<typeof IssueRelations>;

export async function listIssueRelations(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string
): Promise<IssueRelations> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/issue-relation/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(IssueRelations, body, `GET ${path}`);
}

/** One row of the add answer, pointing at the attached issue. */
export const AddedIssueRelation = z.object({
  id: uuid(),
  project_id: uuid(),
  sequence_id: z.number(),
  relation_type: z.string(),
  name: z.string(),
  state_id: nullableUuid(),
  priority: z.enum(ISSUE_PRIORITIES),
  created_by: nullableUuid(),
  created_at: dateTime(),
  updated_by: nullableUuid(),
  updated_at: dateTime(),
});
export type AddedIssueRelation = z.infer<typeof AddedIssueRelation>;

/**
 * Relate several issues to this one under a single type. Answers one
 * row per attached issue. Re-attaching an existing edge is a silent
 * no-op for that pair; a missing type is a 400.
 */
export async function addIssueRelations(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  input: { relationType: IssueRelationType; issues: string[] }
): Promise<AddedIssueRelation[]> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/issue-relation/`;
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: { relation_type: input.relationType, issues: input.issues },
  });
  return client.parse(z.array(AddedIssueRelation), envelope.data, `POST ${path}`);
}

/**
 * Remove the edge between this issue and one related issue, whichever
 * side the row was stored on. Answers 204 with no body.
 */
export async function removeIssueRelation(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  input: { relationType: IssueRelationType; relatedIssue: string }
): Promise<void> {
  await client.post(`/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/remove-relation/`, {
    json: { relation_type: input.relationType, related_issue: input.relatedIssue },
  });
}
