// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Thin module contract for the shell: the command palette's contextual
// edits rename modules. Full module ownership stays in the modules area;
// this is the slice the palette needs to compile and the gate needs to
// prove the rows.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { ArchivedDistribution, ArchivedEstimateDistribution } from "./cycles.js";
import { dateTime, nullableDate, nullableDateTime, nullableUuid, uuid } from "./fields.js";

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

/**
 * Thin workspace module row for cross-project hosts (the workspace
 * view dialog's filter builder). The workspace list is the full
 * module serializer with the project list's favorite annotation
 * missing, so `is_favorite` stays optional and the schema ignores
 * the serializer's remaining keys.
 */
export const WorkspaceModuleRow = z.object({
  id: uuid(),
  project_id: uuid(),
  name: z.string(),
  status: z.string(),
  is_favorite: z.optional(z.boolean()),
});
export type WorkspaceModuleRow = z.infer<typeof WorkspaceModuleRow>;

export const WorkspaceModuleList = z.array(WorkspaceModuleRow);
export type WorkspaceModuleList = z.infer<typeof WorkspaceModuleList>;

/** GET WS/modules/ — workspace-wide rows for switchers and pickers. */
export async function listWorkspaceModules(client: ApiClient, slug: string): Promise<WorkspaceModuleList> {
  const path = `/api/workspaces/${slug}/modules/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(WorkspaceModuleList, body, `GET ${path}`);
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

/**
 * One row of the archived-modules list (ARCH-020). A flat array, newest
 * created first within favorites. Module dates are calendar dates (the
 * model stores DateFields, unlike cycles); the rich-text description
 * columns carry editor JSON or null. The backend selects its columns
 * with .values(), so every key below is always present.
 */
export const ArchivedModuleListItem = z.object({
  id: uuid(),
  workspace_id: uuid(),
  project_id: uuid(),
  name: z.string(),
  description: z.string(),
  description_text: z.nullable(z.unknown()),
  description_html: z.nullable(z.unknown()),
  start_date: nullableDate(),
  target_date: nullableDate(),
  status: z.string(),
  lead_id: nullableUuid(),
  member_ids: z.array(uuid()),
  total_issues: z.number(),
  is_favorite: z.boolean(),
  cancelled_issues: z.number(),
  completed_issues: z.number(),
  started_issues: z.number(),
  unstarted_issues: z.number(),
  backlog_issues: z.number(),
  created_at: dateTime(),
  updated_at: dateTime(),
  archived_at: dateTime(),
});
export type ArchivedModuleListItem = z.infer<typeof ArchivedModuleListItem>;

export const ArchivedModuleList = z.array(ArchivedModuleListItem);
export type ArchivedModuleList = z.infer<typeof ArchivedModuleList>;

/** GET .../archived-modules/: the project's archived modules, flat. */
export async function listArchivedModules(
  client: ApiClient,
  slug: string,
  projectId: string
): Promise<ArchivedModuleList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/archived-modules/`);
  return client.parse(ArchivedModuleList, body, `GET archived modules ${projectId}`);
}

/** One read-only link row on the archived module detail. */
export const ArchivedModuleLink = z.object({
  id: uuid(),
  title: z.nullable(z.string()),
  url: z.string(),
});
export type ArchivedModuleLink = z.infer<typeof ArchivedModuleLink>;

/**
 * Archived module detail for the peek body (ARCH-020): the list columns
 * plus links, the sub-issue count, the per-group estimate sums and the
 * analytics summaries. The backend's extra model columns (view_props,
 * sort_order, external ids, logo) are ignored — the peek never reads
 * them.
 */
export const ArchivedModuleDetail = z.object({
  ...ArchivedModuleListItem.shape,
  link_module: z.array(ArchivedModuleLink),
  sub_issues: z.nullable(z.number()),
  total_estimate_points: z.number(),
  completed_estimate_points: z.number(),
  backlog_estimate_points: z.number(),
  unstarted_estimate_points: z.number(),
  started_estimate_points: z.number(),
  cancelled_estimate_points: z.number(),
  estimate_distribution: ArchivedEstimateDistribution,
  distribution: ArchivedDistribution,
});
export type ArchivedModuleDetail = z.infer<typeof ArchivedModuleDetail>;

/** GET .../archived-modules/<id>/: one archived module for the peek. */
export async function getArchivedModule(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string
): Promise<ArchivedModuleDetail> {
  const body = await client.getJson<unknown>(
    `/api/workspaces/${slug}/projects/${projectId}/archived-modules/${moduleId}/`
  );
  return client.parse(ArchivedModuleDetail, body, `GET archived module ${moduleId}`);
}

/**
 * Module archive stamp: POST .../modules/<id>/archive/ answers 200 with
 * the stamp. Only finished modules archive (status completed or
 * cancelled, otherwise 400), so callers gate the entry on the status.
 * The backend str()s a datetime (space-separated, not ISO), so the
 * stamp stays a plain string like the project one.
 */
export const ModuleArchiveStamp = z.object({
  archived_at: z.string(),
});
export type ModuleArchiveStamp = z.infer<typeof ModuleArchiveStamp>;

export async function archiveModule(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string
): Promise<ModuleArchiveStamp> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/modules/${moduleId}/archive/`;
  const envelope = await client.sendJson<unknown>("POST", path, {});
  return client.parse(ModuleArchiveStamp, envelope.data, `POST ${path}`);
}

/**
 * DELETE .../modules/<id>/archive/ clears the stamp and restores the
 * module. Resolves the status (204); the archived-modules unarchive
 * alias 404s and is deliberately not contracted.
 */
export async function restoreModule(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string
): Promise<number> {
  const response = await client.delete(`/api/workspaces/${slug}/projects/${projectId}/modules/${moduleId}/archive/`);
  return response.status;
}
