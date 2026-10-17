// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue and comment reaction contracts: the toggle model (react posts,
// clicking an existing reaction deletes) plus the lists the detail
// stores refetch after a toggle.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
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
