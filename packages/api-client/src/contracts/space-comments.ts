// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public comment thread (parity/specs/space.md §5.3): list the board's
// EXTERNAL comments, post, edit and delete the visitor's own rows, and
// toggle emoji reactions on a comment. Comment rows are the workspace
// serializer's full shape, so this module keeps only the fields the space
// thread reads and passes the rest through.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { UserLite } from "./users.js";

/** Speaker kinds the public thread attributes a row to. */
export const SPACE_COMMENT_SPEAKERS = ["human", "agent", "system", "integration"] as const;

/**
 * One reaction on a comment row. The public serializer emits the flat
 * actor fields (`actor`, `display_name`) rather than the nested
 * `actor_detail` the issue endpoints use — NEWFRONT-466. The contract
 * declares the flat shape the endpoint actually answers.
 */
export const SpaceCommentReaction = z.object({
  id: uuid(),
  actor: uuid(),
  comment: uuid(),
  reaction: z.string(),
  display_name: z.string(),
  deleted_at: nullableDateTime(),
  workspace: uuid(),
  project: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: z.nullable(z.string()),
  updated_by: z.nullable(z.string()),
});
export type SpaceCommentReaction = z.infer<typeof SpaceCommentReaction>;

/**
 * One comment in the public thread. `labels` carries the fold marker,
 * `comment_reactions` the flat reaction rows, and `edited_at` is an unread
 * passthrough (no space row renders an edited marker).
 */
export const SpaceComment = z.object({
  id: uuid(),
  actor: nullableUuid(),
  actor_detail: z.nullable(UserLite),
  issue: uuid(),
  project: uuid(),
  workspace: uuid(),
  comment_html: z.string(),
  comment_stripped: z.string(),
  comment_json: z.optional(z.unknown()),
  attachments: z.optional(z.array(z.string())),
  labels: z.array(z.string()),
  access: z.string(),
  edited_at: z.optional(nullableDateTime()),
  comment_reactions: z.array(SpaceCommentReaction),
  speaker_type: z.enum(SPACE_COMMENT_SPEAKERS),
  speaker_label: z.string(),
  speaker_agent_run_id: z.optional(nullableUuid()),
  parent: z.optional(nullableUuid()),
  external_source: z.optional(z.nullable(z.string())),
  external_id: z.optional(z.nullable(z.string())),
  is_synced: z.optional(z.boolean()),
  description: z.optional(nullableUuid()),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: z.nullable(z.string()),
  updated_by: z.nullable(z.string()),
  deleted_at: nullableDateTime(),
});
export type SpaceComment = z.infer<typeof SpaceComment>;

export const SpaceCommentList = z.array(SpaceComment);
export type SpaceCommentList = z.infer<typeof SpaceCommentList>;

export async function listSpaceComments(client: ApiClient, anchor: string, issueId: string): Promise<SpaceCommentList> {
  const path = `/api/public/anchor/${encodeURIComponent(anchor)}/issues/${encodeURIComponent(issueId)}/comments/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(SpaceCommentList, body, `GET ${path}`);
}

export interface SpaceCommentCreate {
  commentHtml: string;
  commentJson?: unknown;
}

/**
 * Posts a comment into the public thread. The body carries the image
 * nodes; assets are linked afterwards with the bulk call in
 * space-assets.ts. The server forces `access=EXTERNAL`.
 */
export async function createSpaceComment(
  client: ApiClient,
  anchor: string,
  issueId: string,
  input: SpaceCommentCreate
): Promise<SpaceComment> {
  const path = `/api/public/anchor/${encodeURIComponent(anchor)}/issues/${encodeURIComponent(issueId)}/comments/`;
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: {
      comment_html: input.commentHtml,
      ...(input.commentJson === undefined ? {} : { comment_json: input.commentJson }),
      access: "EXTERNAL",
    },
  });
  return client.parse(SpaceComment, envelope.data, `POST ${path}`);
}

export interface SpaceCommentPatch {
  commentHtml?: string;
  commentJson?: unknown;
}

export async function updateSpaceComment(
  client: ApiClient,
  anchor: string,
  issueId: string,
  commentId: string,
  patch: SpaceCommentPatch
): Promise<SpaceComment> {
  const path = `/api/public/anchor/${encodeURIComponent(anchor)}/issues/${encodeURIComponent(issueId)}/comments/${encodeURIComponent(commentId)}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, {
    json: {
      ...(patch.commentHtml === undefined ? {} : { comment_html: patch.commentHtml }),
      ...(patch.commentJson === undefined ? {} : { comment_json: patch.commentJson }),
    },
  });
  return client.parse(SpaceComment, envelope.data, `PATCH ${path}`);
}

/** Deletes the visitor's own comment. Answers 204. */
export async function deleteSpaceComment(
  client: ApiClient,
  anchor: string,
  issueId: string,
  commentId: string
): Promise<void> {
  await client.delete(
    `/api/public/anchor/${encodeURIComponent(anchor)}/issues/${encodeURIComponent(issueId)}/comments/${encodeURIComponent(commentId)}/`
  );
}

/** Adds one emoji reaction to a comment; answers the flat reaction row. */
export async function addSpaceCommentReaction(
  client: ApiClient,
  anchor: string,
  commentId: string,
  reaction: string
): Promise<SpaceCommentReaction> {
  const path = `/api/public/anchor/${encodeURIComponent(anchor)}/comments/${encodeURIComponent(commentId)}/reactions/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { reaction } });
  return client.parse(SpaceCommentReaction, envelope.data, `POST ${path}`);
}

/** Removes the visitor's reaction by code. Answers 204. */
export async function removeSpaceCommentReaction(
  client: ApiClient,
  anchor: string,
  commentId: string,
  reaction: string
): Promise<void> {
  await client.delete(
    `/api/public/anchor/${encodeURIComponent(anchor)}/comments/${encodeURIComponent(commentId)}/reactions/${encodeURIComponent(reaction)}/`
  );
}
