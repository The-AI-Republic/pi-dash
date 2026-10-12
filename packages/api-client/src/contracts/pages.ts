// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Thin page (document) contract for the shell: the command palette's
// contextual edits rename pages. Full page ownership stays in the pages
// area; this is the slice the palette needs to compile and the gate needs
// to prove the rows.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableDateTime, uuid } from "./fields.js";

export const PageUpdate = z.object({
  id: uuid(),
  name: z.string(),
  access: z.optional(z.number()),
  color: z.optional(z.string()),
  is_locked: z.optional(z.boolean()),
  archived_at: z.optional(nullableDateTime()),
  project_ids: z.optional(z.array(uuid())),
});
export type PageUpdate = z.infer<typeof PageUpdate>;

export interface PagePatch {
  name?: string;
  color?: string;
  is_locked?: boolean;
}

export async function updatePage(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string,
  input: PagePatch
): Promise<PageUpdate> {
  const envelope = await client.sendJson<unknown>(
    "PATCH",
    `/api/workspaces/${slug}/projects/${projectId}/pages/${pageId}/`,
    { json: input }
  );
  return client.parse(PageUpdate, envelope.data, `PATCH page ${pageId}`);
}
