// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Code-review contracts: the per-issue review collection with its
// attach and detach writes. The client only ever supplies a PR or MR
// URL; every status field is server-managed. Re-attaching the same
// URL to the same issue answers the row again instead of duplicating
// it; attaching it to another issue is a 409.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { LinkAuthor } from "./issue-links.js";

/** Review states the server reports for a linked review. */
export const CODE_REVIEW_STATES = ["open", "closed", "merged"] as const;
export type CodeReviewState = (typeof CODE_REVIEW_STATES)[number];

/** One review linked to an issue, newest first. */
export const CodeReviewLink = z.object({
  id: uuid(),
  issue: uuid(),
  provider: z.string(),
  host_url: z.string(),
  namespace: z.string(),
  repo_name: z.string(),
  repo_external_id: z.string(),
  external_id: z.string(),
  external_iid: z.string(),
  url: z.string(),
  title: z.string(),
  state: z.enum(CODE_REVIEW_STATES),
  merged: z.boolean(),
  draft: z.boolean(),
  remote_updated_at: nullableDateTime(),
  metadata: z.record(z.string(), z.unknown()),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  created_by_detail: z.nullable(LinkAuthor),
});
export type CodeReviewLink = z.infer<typeof CodeReviewLink>;

export const CodeReviewLinkList = z.array(CodeReviewLink);
export type CodeReviewLinkList = z.infer<typeof CodeReviewLinkList>;

export async function listCodeReviews(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string
): Promise<CodeReviewLinkList> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/code-reviews/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(CodeReviewLinkList, body, `GET ${path}`);
}

export async function attachCodeReview(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  url: string
): Promise<CodeReviewLink> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/code-reviews/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { url } });
  return client.parse(CodeReviewLink, envelope.data, `POST ${path}`);
}

export async function detachCodeReview(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  linkId: string
): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/code-reviews/${linkId}/`);
}
