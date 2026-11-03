// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue and comment reaction contracts: the toggle model (react posts,
// clicking an existing reaction deletes) plus the lists the detail
// stores refetch after a toggle.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { isApiError } from "../errors.js";
import { dateTime, nullableDateTime, uuid } from "./fields.js";
import { UserLite } from "./users.js";

/**
 * One issue reaction. The row carries the reactor inline so the row can
 * highlight the current user and tooltip reactor names without a join.
 */
export const IssueReaction = z.object({
  id: uuid(),
  actor: uuid(),
  actor_detail: UserLite,
  issue: uuid(),
  reaction: z.string(),
  project: uuid(),
  workspace: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: z.nullable(z.string()),
  updated_by: z.nullable(z.string()),
  deleted_at: nullableDateTime(),
});
export type IssueReaction = z.infer<typeof IssueReaction>;

export const IssueReactionList = z.array(IssueReaction);
export type IssueReactionList = z.infer<typeof IssueReactionList>;

export async function listIssueReactions(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string
): Promise<IssueReactionList> {
  const body = await client.getJson<unknown>(
    `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/reactions/`
  );
  return client.parse(IssueReactionList, body, `GET issue reactions ${issueId}`);
}

export async function createIssueReaction(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  reaction: string
): Promise<IssueReaction> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/reactions/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { reaction } });
  return client.parse(IssueReaction, envelope.data, `POST ${path}`);
}

export async function deleteIssueReaction(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  reaction: string
): Promise<void> {
  await client.delete(
    `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/reactions/${encodeURIComponent(reaction)}/`
  );
}

/**
 * One comment reaction. Unlike the issue rows these carry only the
 * reactor's display name — comment footers tooltip names, never avatars.
 */
export const CommentReaction = z.object({
  id: uuid(),
  actor: uuid(),
  comment: uuid(),
  reaction: z.string(),
  display_name: z.string(),
  project: uuid(),
  workspace: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: z.nullable(z.string()),
  updated_by: z.nullable(z.string()),
  deleted_at: nullableDateTime(),
});
export type CommentReaction = z.infer<typeof CommentReaction>;

export const CommentReactionList = z.array(CommentReaction);
export type CommentReactionList = z.infer<typeof CommentReactionList>;

export async function listCommentReactions(
  client: ApiClient,
  slug: string,
  projectId: string,
  commentId: string
): Promise<CommentReactionList> {
  const body = await client.getJson<unknown>(
    `/api/workspaces/${slug}/projects/${projectId}/comments/${commentId}/reactions/`
  );
  return client.parse(CommentReactionList, body, `GET comment reactions ${commentId}`);
}

export async function createCommentReaction(
  client: ApiClient,
  slug: string,
  projectId: string,
  commentId: string,
  reaction: string
): Promise<CommentReaction> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/comments/${commentId}/reactions/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { reaction } });
  return client.parse(CommentReaction, envelope.data, `POST ${path}`);
}

export async function deleteCommentReaction(
  client: ApiClient,
  slug: string,
  projectId: string,
  commentId: string,
  reaction: string
): Promise<void> {
  await client.delete(
    `/api/workspaces/${slug}/projects/${projectId}/comments/${commentId}/reactions/${encodeURIComponent(reaction)}/`
  );
}

/** The duplicate-add refusal body: 400 with a single error sentence. */
export const DuplicateReactionErrorBody = z.object({
  error: z.string(),
});
export type DuplicateReactionErrorBody = z.infer<typeof DuplicateReactionErrorBody>;

/**
 * The issue path's duplicate sentence. The comment view names the
 * duplicate itself; the issue view lets the base viewset translate the
 * unique-constraint violation into this generic payload message.
 */
export const ISSUE_DUPLICATE_REACTION_MESSAGE = "The payload is not valid";

export interface DuplicateReactionFailure {
  /** The server's duplicate sentence. */
  message: string;
  /** Always 400: the duplicate refusal status. */
  status: number;
}

/**
 * Read the duplicate-reaction refusal off a rejected reaction create:
 * the 400 plus the duplicate sentence, on either the comment path
 * ("already exists") or the issue path (the base payload message).
 * Null when the failure is not a duplicate refusal (transport errors,
 * contract violations, other statuses, or another 400 shape such as
 * serializer field errors), so callers branch on the result instead of
 * matching the server text themselves. Only call with errors from
 * {@link createIssueReaction} and {@link createCommentReaction}.
 */
export function duplicateReactionErrorFrom(error: unknown): DuplicateReactionFailure | null {
  if (!isApiError(error) || error.status !== 400) return null;
  const parsed = DuplicateReactionErrorBody.safeParse(error.body);
  if (!parsed.success) return null;
  const message = parsed.data.error;
  if (!/already exists/i.test(message) && message !== ISSUE_DUPLICATE_REACTION_MESSAGE) return null;
  return { message, status: error.status };
}
