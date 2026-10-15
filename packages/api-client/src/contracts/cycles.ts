// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Thin cycle contract for the shell: the command palette's contextual edits
// rename and reschedule cycles. Full cycle ownership stays in the cycles
// area; this is the slice the palette needs to compile and the gate needs
// to prove the rows. PATCH applies the edit and answers the updated
// representation.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableDateTime, uuid } from "./fields.js";

export const CycleUpdate = z.object({
  id: uuid(),
  name: z.string(),
  project_id: uuid(),
  description: z.optional(z.string()),
  start_date: nullableDateTime(),
  end_date: nullableDateTime(),
});
export type CycleUpdate = z.infer<typeof CycleUpdate>;

/**
 * Thin cycle row for the shell: the palette's Add-to-cycle picker lists
 * names. The server already excludes archived cycles. Full cycle
 * ownership stays in the cycles area.
 */
export const CycleListItem = z.object({
  id: uuid(),
  project_id: uuid(),
  name: z.string(),
  is_favorite: z.boolean(),
});
export type CycleListItem = z.infer<typeof CycleListItem>;

export const CycleList = z.array(CycleListItem);
export type CycleList = z.infer<typeof CycleList>;

export async function listCycles(client: ApiClient, slug: string, projectId: string): Promise<CycleList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/cycles/`);
  return client.parse(CycleList, body, `GET cycles ${projectId}`);
}

export interface CyclePatch {
  name?: string;
  description?: string;
  start_date?: string | null;
  end_date?: string | null;
}

export async function updateCycle(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string,
  input: CyclePatch
): Promise<CycleUpdate> {
  const envelope = await client.sendJson<unknown>(
    "PATCH",
    `/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/`,
    { json: input }
  );
  return client.parse(CycleUpdate, envelope.data, `PATCH cycle ${cycleId}`);
}
