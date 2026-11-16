// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public engagement writes (parity/specs/space.md §5.2): issue votes and
// issue emoji reactions. Both are toggles — POST creates, DELETE removes —
// and both need a signed-in visitor. The board reads state from the issue
// row's embedded `vote_items`/`reaction_items` (space-issues.ts); the
// collection reads exist but answer empty for anonymous callers because
// they resolve the board by the wrong key, so the app does not use them.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, uuid } from "./fields.js";
import { UserLite } from "./users.js";

/** Vote values the UI sends: +1 up, -1 down. */
export const SPACE_VOTE_VALUES = [1, -1] as const;
export type SpaceVoteValue = (typeof SPACE_VOTE_VALUES)[number];

/** The stored vote a POST answers. */
export const SpaceVote = z.object({
  issue: uuid(),
  vote: z.number(),
  workspace: uuid(),
  project: uuid(),
  actor: uuid(),
  actor_detail: UserLite,
});
export type SpaceVote = z.infer<typeof SpaceVote>;

export async function setSpaceIssueVote(
  client: ApiClient,
  anchor: string,
  issueId: string,
  vote: SpaceVoteValue
): Promise<SpaceVote> {
  const path = `/api/public/anchor/${encodeURIComponent(anchor)}/issues/${encodeURIComponent(issueId)}/votes/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { vote } });
  return client.parse(SpaceVote, envelope.data, `POST ${path}`);
}

/** Removes the signed-in visitor's vote. Answers 204. */
export async function removeSpaceIssueVote(client: ApiClient, anchor: string, issueId: string): Promise<void> {
  await client.delete(`/api/public/anchor/${encodeURIComponent(anchor)}/issues/${encodeURIComponent(issueId)}/votes/`);
}

/** The stored issue reaction a POST answers. */
export const SpaceIssueReaction = z.object({
  id: uuid(),
  actor_detail: UserLite,
  created_at: dateTime(),
  updated_at: dateTime(),
  deleted_at: nullableDateTime(),
  reaction: z.string(),
  created_by: z.nullable(z.string()),
  updated_by: z.nullable(z.string()),
  project: uuid(),
  workspace: uuid(),
  actor: uuid(),
  issue: uuid(),
});
export type SpaceIssueReaction = z.infer<typeof SpaceIssueReaction>;

export async function addSpaceIssueReaction(
  client: ApiClient,
  anchor: string,
  issueId: string,
  reaction: string
): Promise<SpaceIssueReaction> {
  const path = `/api/public/anchor/${encodeURIComponent(anchor)}/issues/${encodeURIComponent(issueId)}/reactions/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { reaction } });
  return client.parse(SpaceIssueReaction, envelope.data, `POST ${path}`);
}

/** Removes one emoji reaction by its code. Answers 204. */
export async function removeSpaceIssueReaction(
  client: ApiClient,
  anchor: string,
  issueId: string,
  reaction: string
): Promise<void> {
  await client.delete(
    `/api/public/anchor/${encodeURIComponent(anchor)}/issues/${encodeURIComponent(issueId)}/reactions/${encodeURIComponent(reaction)}/`
  );
}
