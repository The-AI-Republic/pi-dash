// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Project contracts (GET /api/workspaces/<slug>/projects/).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableDateTime, nullableUuid, uuid } from "./fields.js";

/** Small project projection embedded in members and activity. */
export const ProjectLite = z.object({
  id: uuid(),
  name: z.string(),
  identifier: z.string(),
});
export type ProjectLite = z.infer<typeof ProjectLite>;

/**
 * Project list row. The endpoint answers a fixed `.values()` projection —
 * not the full serializer — with the caller's membership role, the pending
 * intake count and the view flags annotated. Anything else the server adds
 * passes through untouched.
 */
export const Project = z.object({
  id: uuid(),
  name: z.string(),
  identifier: z.string(),
  sort_order: z.nullable(z.number()),
  logo_props: z.record(z.string(), z.unknown()),
  member_role: z.nullable(z.number()),
  intake_count: z.number(),
  archived_at: nullableDateTime(),
  workspace: uuid(),
  cycle_view: z.boolean(),
  issue_views_view: z.boolean(),
  module_view: z.boolean(),
  page_view: z.boolean(),
  inbox_view: z.boolean(),
  is_default: z.boolean(),
  guest_view_all_features: z.boolean(),
  project_lead: nullableUuid(),
  network: z.number(),
  created_at: z.iso.datetime(),
  updated_at: z.iso.datetime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
});
export type Project = z.infer<typeof Project>;

export const ProjectList = z.array(Project);
export type ProjectList = z.infer<typeof ProjectList>;

export async function listProjects(client: ApiClient, slug: string): Promise<ProjectList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/`);
  return client.parse(ProjectList, body, `GET /api/workspaces/${slug}/projects/`);
}
