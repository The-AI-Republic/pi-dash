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
