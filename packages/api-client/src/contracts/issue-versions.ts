// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Description-version contracts: the per-issue version history read
// plus the single-version read the restore flow loads. Versions are
// server-written on description saves; there is no write here.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableUuid, uuid } from "./fields.js";

/** One version row: identity plus who saved it and when. */
export const DescriptionVersion = z.object({
  id: uuid(),
  workspace: uuid(),
  project: uuid(),
  issue: uuid(),
  last_saved_at: dateTime(),
  owned_by: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
});
export type DescriptionVersion = z.infer<typeof DescriptionVersion>;

/** Page envelope for the version history, newest first. */
export const DescriptionVersionListResponse = z.object({
  prev_cursor: z.string(),
  cursor: z.string(),
  next_cursor: z.nullable(z.string()),
  prev_page_results: z.boolean(),
  next_page_results: z.boolean(),
  page_count: z.number(),
  total_results: z.number(),
  total_pages: z.number(),
  results: z.array(DescriptionVersion),
});
export type DescriptionVersionListResponse = z.infer<typeof DescriptionVersionListResponse>;

export async function listDescriptionVersions(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  query: { cursor?: string } = {}
): Promise<DescriptionVersionListResponse> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/work-items/${issueId}/description-versions/`;
  const body = await client.getJson<unknown>(path, { query: { cursor: query.cursor } });
  return client.parse(DescriptionVersionListResponse, body, `GET ${path}`);
}

/** One version with its stored description payloads. */
export const DescriptionVersionDetail = z.object({
  id: uuid(),
  workspace: uuid(),
  project: uuid(),
  issue: uuid(),
  description_binary: z.nullable(z.string()),
  description_html: z.string(),
  description_stripped: z.nullable(z.string()),
  description_json: z.record(z.string(), z.unknown()),
  last_saved_at: dateTime(),
  owned_by: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
});
export type DescriptionVersionDetail = z.infer<typeof DescriptionVersionDetail>;

export async function getDescriptionVersion(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  versionId: string
): Promise<DescriptionVersionDetail> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/work-items/${issueId}/description-versions/${versionId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(DescriptionVersionDetail, body, `GET ${path}`);
}
