// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue comment contracts. The list reads through the history endpoint
// with the issue-comment activity type (the comments collection itself
// only serves writes plus single-row reads); create, edit, access
// toggle, fold-label writes and delete run against the collection.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { isApiError } from "../errors.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { UserLite } from "./users.js";
import { CommentReaction } from "./issue-reactions.js";

/** Internal/external access specifier carried on every comment. */
export const COMMENT_ACCESS = ["INTERNAL", "EXTERNAL"] as const;
export type CommentAccess = (typeof COMMENT_ACCESS)[number];

/** Fold marker stored in the comment's labels array (ISS-201). */
export const COMMENT_FOLD_LABEL = "fold";

/** Speaker kinds for conversation attribution in the thread. */
export const COMMENT_SPEAKERS = ["human", "agent", "system", "integration"] as const;
export type CommentSpeaker = (typeof COMMENT_SPEAKERS)[number];

/**
 * One issue comment. The server embeds the actor plus the comment's own
 * reactions; issue/project/workspace details ride on the response too but
 * slices already hold those rows, so they stay stripped here.
 */
export const IssueComment = z.object({
  id: uuid(),
  issue: uuid(),
  project: uuid(),
  workspace: uuid(),
  actor: nullableUuid(),
  actor_detail: z.nullable(UserLite),
  comment_html: z.string(),
  comment_stripped: z.string(),
  comment_json: z.unknown(),
  attachments: z.array(z.string()),
  labels: z.array(z.string()),
  access: z.enum(COMMENT_ACCESS),
  edited_at: nullableDateTime(),
  comment_reactions: z.array(CommentReaction),
  speaker_type: z.enum(COMMENT_SPEAKERS),
  speaker_label: z.string(),
  speaker_agent_run_id: nullableUuid(),
  parent: nullableUuid(),
  external_source: z.nullable(z.string()),
  external_id: z.nullable(z.string()),
  is_synced: z.boolean(),
  description: nullableUuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: z.nullable(z.string()),
  updated_by: z.nullable(z.string()),
  deleted_at: nullableDateTime(),
});
export type IssueComment = z.infer<typeof IssueComment>;

export const IssueCommentList = z.array(IssueComment);
export type IssueCommentList = z.infer<typeof IssueCommentList>;

export interface IssueCommentQuery {
  /** Only rows created after this timestamp (the feed's refresh cursor). */
  createdAfter?: string;
}

export async function listIssueComments(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  query: IssueCommentQuery = {}
): Promise<IssueCommentList> {
  const body = await client.getJson<unknown>(
    `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/history/`,
    {
      query: {
        activity_type: "issue-comment",
        created_at__gt: query.createdAfter,
      },
    }
  );
  return client.parse(IssueCommentList, body, `GET issue comments ${issueId}`);
}

export interface IssueCommentCreate {
  comment_html: string;
  comment_json?: unknown;
  attachments?: string[];
  access?: CommentAccess;
}

export async function createIssueComment(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  input: IssueCommentCreate
): Promise<IssueComment> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/comments/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(IssueComment, envelope.data, `POST ${path}`);
}

export interface IssueCommentPatch {
  comment_html?: string;
  comment_json?: unknown;
  attachments?: string[];
  access?: CommentAccess;
  labels?: string[];
}

export async function updateIssueComment(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  commentId: string,
  input: IssueCommentPatch
): Promise<IssueComment> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/comments/${commentId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(IssueComment, envelope.data, `PATCH ${path}`);
}

export async function deleteIssueComment(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  commentId: string
): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/comments/${commentId}/`);
}

/** The git-synced delete refusal body: 409 with an unbind instruction. */
export const GitSyncConflictErrorBody = z.object({
  error: z.string(),
});
export type GitSyncConflictErrorBody = z.infer<typeof GitSyncConflictErrorBody>;

/**
 * The synced-body edit refusal: 400 naming each locked body field
 * (`comment_html`, `comment_json`, `comment_stripped`) whose value the
 * PATCH tried to change, each carrying the read-only sentence.
 */
export const GitSyncLockedFieldsErrorBody = z.record(z.string(), z.array(z.string()));
export type GitSyncLockedFieldsErrorBody = z.infer<typeof GitSyncLockedFieldsErrorBody>;

export interface GitSyncedCommentFailure {
  /** The server's instruction (names unbinding the repository). */
  message: string;
  /** 409 for a refused delete, 400 for a refused body edit. */
  status: number;
  /** Locked fields the edit touched; empty on the delete path. */
  fields: string[];
}

/**
 * Read the git-sync refusal off a rejected comment write: the unbind
 * instruction, the HTTP status, and (on edits) the locked fields. Null
 * when the failure is not a git-sync refusal (transport errors,
 * contract violations, other statuses, or bodies without the unbind
 * marker), so callers branch on the result instead of matching the
 * server text themselves.
 */
export function gitSyncedCommentErrorFrom(error: unknown): GitSyncedCommentFailure | null {
  if (!isApiError(error)) return null;
  if (error.status === 409) {
    const parsed = GitSyncConflictErrorBody.safeParse(error.body);
    if (!parsed.success || !/unbind/i.test(parsed.data.error)) return null;
    return { message: parsed.data.error, status: error.status, fields: [] };
  }
  if (error.status === 400) {
    const parsed = GitSyncLockedFieldsErrorBody.safeParse(error.body);
    if (!parsed.success) return null;
    const fields: string[] = [];
    let message: string | undefined;
    for (const [field, messages] of Object.entries(parsed.data)) {
      const hit = messages.find((entry) => /unbind/i.test(entry));
      if (hit !== undefined) {
        fields.push(field);
        message ??= hit;
      }
    }
    if (message === undefined) return null;
    return { message, status: error.status, fields };
  }
  return null;
}
