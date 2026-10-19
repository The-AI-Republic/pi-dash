// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue activity contract: the property-change half of the Activity
// timeline. The feed merges these rows with comments client-side and
// applies its Updates/Comments/State/Assignee filter plus the asc/desc
// sort locally, so the read takes only the refresh cursor.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { UserLite } from "./users.js";

/**
 * The issue a history row points at, flattened as the history endpoints
 * carry it. Absent when the issue was deleted (the row keeps its id but
 * there is nothing to link or name).
 */
export const ActivityIssueRef = z.object({
  id: uuid(),
  name: z.string(),
  sequence_id: z.number(),
});
export type ActivityIssueRef = z.infer<typeof ActivityIssueRef>;

/** The project a history row points at, flattened for its identifier. */
export const ActivityProjectRef = z.object({
  id: uuid(),
  identifier: z.string(),
  name: z.string(),
});
export type ActivityProjectRef = z.infer<typeof ActivityProjectRef>;

/** The workspace a row belongs to; the own-history feed spans workspaces. */
export const ActivityWorkspaceRef = z.object({
  id: uuid(),
  name: z.string(),
  slug: z.string(),
});
export type ActivityWorkspaceRef = z.infer<typeof ActivityWorkspaceRef>;

/**
 * One property-change activity row. The feed paraphrases field plus
 * old/new values into a sentence; every field the server sends renders.
 */
export const IssueActivity = z.object({
  id: uuid(),
  issue: nullableUuid(),
  project: uuid(),
  workspace: uuid(),
  actor: nullableUuid(),
  actor_detail: z.nullable(UserLite),
  /**
   * Flat work-item projection the workspace user-activity feed serialises
   * beside the row (id, name and sequence id). The issue-detail feed omits
   * it, so readers treat absence as "no key to link".
   */
  issue_detail: z.optional(z.nullable(z.object({ id: uuid(), name: z.string(), sequence_id: z.number() }))),
  /** Project projection carrying the identifier the browse key needs. */
  project_detail: z.optional(z.nullable(z.object({ id: uuid(), identifier: z.string(), name: z.string() }))),
  verb: z.string(),
  field: z.nullable(z.string()),
  old_value: z.nullable(z.string()),
  new_value: z.nullable(z.string()),
  comment: z.string(),
  attachments: z.array(z.string()),
  issue_comment: nullableUuid(),
  old_identifier: nullableUuid(),
  new_identifier: nullableUuid(),
  epoch: z.nullable(z.number()),
  source_data: z.unknown(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: z.nullable(z.string()),
  updated_by: z.nullable(z.string()),
  deleted_at: nullableDateTime(),
  /** Flattened link targets; the profile feeds need them to name rows. */
  issue_detail: z.nullable(ActivityIssueRef),
  project_detail: z.nullable(ActivityProjectRef),
  workspace_detail: z.nullable(ActivityWorkspaceRef),
});
export type IssueActivity = z.infer<typeof IssueActivity>;

export const IssueActivityList = z.array(IssueActivity);
export type IssueActivityList = z.infer<typeof IssueActivityList>;

export interface IssueActivityQuery {
  /** Only rows created after this timestamp (the feed's refresh cursor). */
  createdAfter?: string;
}

export async function listIssueActivities(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  query: IssueActivityQuery = {}
): Promise<IssueActivityList> {
  const body = await client.getJson<unknown>(
    `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/history/`,
    {
      query: {
        activity_type: "issue-property",
        created_at__gt: query.createdAfter,
      },
    }
  );
  return client.parse(IssueActivityList, body, `GET issue activities ${issueId}`);
}
