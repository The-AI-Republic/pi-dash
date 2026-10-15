// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Thin module contract for the shell: the command palette's contextual
// edits rename modules. Full module ownership stays in the modules area;
// this is the slice the palette needs to compile and the gate needs to
// prove the rows.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableDateTime, uuid } from "./fields.js";

export const ModuleUpdate = z.object({
  id: uuid(),
  name: z.string(),
  project_id: uuid(),
  description: z.optional(z.string()),
  start_date: nullableDateTime(),
  target_date: nullableDateTime(),
});
export type ModuleUpdate = z.infer<typeof ModuleUpdate>;

export interface ModulePatch {
  name?: string;
  description?: string;
  start_date?: string | null;
  target_date?: string | null;
  status?: string;
  member_ids?: string[];
}

/** Module workflow states offered by the palette's Change-status picker. */
export const MODULE_STATUSES = ["backlog", "planned", "in-progress", "paused", "completed", "cancelled"] as const;
export type ModuleStatus = (typeof MODULE_STATUSES)[number];

/**
 * Thin module row for the shell: the palette's Add-to-modules picker
 * lists names. The server already excludes archived modules. Full
 * module ownership stays in the modules area.
 */
export const ModuleListItem = z.object({
  id: uuid(),
  project_id: uuid(),
  name: z.string(),
  status: z.string(),
  is_favorite: z.boolean(),
});
export type ModuleListItem = z.infer<typeof ModuleListItem>;

export const ModuleList = z.array(ModuleListItem);
export type ModuleList = z.infer<typeof ModuleList>;

export async function listModules(client: ApiClient, slug: string, projectId: string): Promise<ModuleList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/modules/`);
  return client.parse(ModuleList, body, `GET modules ${projectId}`);
}

export async function updateModule(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string,
  input: ModulePatch
): Promise<ModuleUpdate> {
  const envelope = await client.sendJson<unknown>(
    "PATCH",
    `/api/workspaces/${slug}/projects/${projectId}/modules/${moduleId}/`,
    { json: input }
  );
  return client.parse(ModuleUpdate, envelope.data, `PATCH module ${moduleId}`);
}
