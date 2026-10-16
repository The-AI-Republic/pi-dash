// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue lifecycle: archive/restore and move-to-project. Duplicating has no
// endpoint — the app re-posts the source fields through createIssue — so
// there is deliberately no duplicate function here.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDate, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { ISSUE_PRIORITIES } from "./issues.js";

/**
 * Archive stamp: POST .../archive/ answers 200 with the date it stamped.
 * Only issues in a completed or cancelled state archive (otherwise 400),
 * so callers gate the entry on the state group.
 */
export const IssueArchiveResult = z.object({
  archived_at: z.iso.date(),
});
export type IssueArchiveResult = z.infer<typeof IssueArchiveResult>;

export async function archiveIssue(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string
): Promise<IssueArchiveResult> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/archive/`;
  const envelope = await client.sendJson<unknown>("POST", path, {});
  return client.parse(IssueArchiveResult, envelope.data, `POST ${path}`);
}

/** DELETE .../archive/ clears the stamp (204) and restores the issue. */
export async function restoreIssue(client: ApiClient, slug: string, projectId: string, issueId: string): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/archive/`);
}

/**
 * The moved issue, as the move endpoint returns it: the read
 * serializer's field set (no description or subscription state —
 * callers re-read the detail when they need those). The endpoint
 * serializes a bare instance, so the annotation-backed keys (cycle,
 * membership arrays, counts) are absent rather than null.
 */
export const MovedIssue = z.object({
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
  cycle_id: z.optional(nullableUuid()),
  assigned_pod_id: z.optional(nullableUuid()),
  agent_executor: z.nullable(z.string()),
  module_ids: z.optional(z.array(uuid())),
  label_ids: z.optional(z.array(uuid())),
  assignee_ids: z.optional(z.array(uuid())),
  sub_issues_count: z.optional(z.nullable(z.number())),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: z.nullable(z.string()),
  updated_by: z.nullable(z.string()),
  attachment_count: z.optional(z.nullable(z.number())),
  link_count: z.optional(z.nullable(z.number())),
  is_draft: z.boolean(),
  archived_at: nullableDate(),
  is_synced: z.boolean(),
});
export type MovedIssue = z.infer<typeof MovedIssue>;

/**
 * POST .../work-items/<id>/move/ — move the issue into another project
 * of the same workspace. The target takes a project UUID or its
 * workspace-scoped identifier; the issue lands in the target's default
 * state with a fresh sequence id, so callers re-read and redirect.
 * Cross-workspace moves are unsupported (400).
 */
export async function moveIssueToProject(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  targetProject: string
): Promise<MovedIssue> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/work-items/${issueId}/move/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { project: targetProject } });
  return client.parse(MovedIssue, envelope.data, `POST ${path}`);
}
