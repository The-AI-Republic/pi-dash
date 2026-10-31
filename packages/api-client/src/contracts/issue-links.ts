// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// External-link contracts: the per-issue link collection with its
// add, edit and remove writes. The server prefixes a missing URL
// scheme and rejects malformed or duplicate URLs per issue.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";

/** The compact author projection link rows carry. */
export const LinkAuthor = z.object({
  id: uuid(),
  first_name: z.string(),
  last_name: z.string(),
  avatar: z.string(),
  avatar_url: z.nullable(z.string()),
  is_bot: z.boolean(),
  display_name: z.string(),
});
export type LinkAuthor = z.infer<typeof LinkAuthor>;

/** One external link on an issue, newest first. */
export const IssueLink = z.object({
  id: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  deleted_at: nullableDateTime(),
  project: uuid(),
  workspace: uuid(),
  title: z.nullable(z.string()),
  url: z.string(),
  issue: uuid(),
  metadata: z.record(z.string(), z.unknown()),
  created_by_detail: z.nullable(LinkAuthor),
});
export type IssueLink = z.infer<typeof IssueLink>;

export const IssueLinkList = z.array(IssueLink);
export type IssueLinkList = z.infer<typeof IssueLinkList>;

export interface IssueLinkInput {
  url: string;
  title?: string | null;
}

export async function listIssueLinks(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string
): Promise<IssueLinkList> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/issue-links/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(IssueLinkList, body, `GET ${path}`);
}

export async function createIssueLink(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  input: IssueLinkInput
): Promise<IssueLink> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/issue-links/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(IssueLink, envelope.data, `POST ${path}`);
}

export async function updateIssueLink(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  linkId: string,
  input: Partial<IssueLinkInput>
): Promise<IssueLink> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/issue-links/${linkId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(IssueLink, envelope.data, `PATCH ${path}`);
}

export async function deleteIssueLink(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  linkId: string
): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/issue-links/${linkId}/`);
}
