// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace contracts: the workspaces the user belongs to, plus the small
// projection embedded in projects and members.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { uuid } from "./fields.js";

export const WorkspaceLite = z.object({
  id: uuid(),
  name: z.string(),
  slug: z.string(),
  logo_url: z.nullable(z.string()),
});
export type WorkspaceLite = z.infer<typeof WorkspaceLite>;

/** One workspace row with the caller's role and member count annotated. */
export const Workspace = z.object({
  id: uuid(),
  name: z.string(),
  slug: z.string(),
  logo_url: z.nullable(z.string()),
  total_members: z.number(),
  role: z.number(),
});
export type Workspace = z.infer<typeof Workspace>;

export const WorkspaceList = z.array(Workspace);
export type WorkspaceList = z.infer<typeof WorkspaceList>;

export async function listWorkspaces(client: ApiClient): Promise<WorkspaceList> {
  const body = await client.getJson<unknown>("/api/users/me/workspaces/");
  return client.parse(WorkspaceList, body, "GET /api/users/me/workspaces/");
}
