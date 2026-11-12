// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue identity-resolution reads: the legacy short-link meta lookup
// plus the lite identifier fetch that opens the browse route. The full
// hydrate that follows goes through the covered issue detail.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDate, uuid } from "./fields.js";

/**
 * Short-link meta: the human identifier pair the legacy
 * /{ws}/projects/{p}/issues/{id} loader redirects with.
 */
export const IssueMeta = z.object({
  sequence_id: z.number(),
  project_identifier: z.string(),
});
export type IssueMeta = z.infer<typeof IssueMeta>;

export async function getIssueMeta(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string
): Promise<IssueMeta> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/issues/${issueId}/meta/`);
  return client.parse(IssueMeta, body, `GET issue meta ${issueId}`);
}

/**
 * Lite identifier row: id plus project for the route, flags for the
 * intake redirect and archived/synced handling. Everything else comes
 * from the full hydrate.
 */
export const IssueLite = z.object({
  id: uuid(),
  sequence_id: z.number(),
  name: z.string(),
  description_html: z.string(),
  sort_order: z.number(),
  project_id: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: z.nullable(z.string()),
  updated_by: z.nullable(z.string()),
  is_draft: z.boolean(),
  is_epic: z.boolean(),
  is_intake: z.boolean(),
  is_synced: z.boolean(),
  archived_at: nullableDate(),
});
export type IssueLite = z.infer<typeof IssueLite>;

export async function getIssueByIdentifier(
  client: ApiClient,
  slug: string,
  projectIdentifier: string,
  sequenceId: number
): Promise<IssueLite> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/work-items/${projectIdentifier}-${sequenceId}/`, {
    query: { lite: true },
  });
  return client.parse(IssueLite, body, `GET lite ${projectIdentifier}-${sequenceId}`);
}
