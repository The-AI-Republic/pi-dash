// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-context display preferences: the project, workspace, cycle, module
// and epics user-properties records each context's filters, layout and
// visible properties. Project and workspace record reads live in
// projects.ts; saved views carry the same payload on the view record
// (issue-views.ts).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableUuid, uuid } from "./fields.js";
import { ProjectUserProperties, WorkspaceUserProperties } from "./projects.js";

/** The four filter keys every user-properties record carries. */
export interface PrefsPatch {
  filters?: Record<string, unknown>;
  rich_filters?: Record<string, unknown>;
  display_filters?: Record<string, unknown>;
  display_properties?: Record<string, unknown>;
}

const prefsFields = {
  filters: z.record(z.string(), z.unknown()),
  display_filters: z.record(z.string(), z.unknown()),
  display_properties: z.record(z.string(), z.unknown()),
  rich_filters: z.record(z.string(), z.unknown()),
};

/**
 * Cycle display properties for the caller. GET creates the record on
 * first read (200); PATCH replaces the given keys wholesale and answers
 * 201, so callers send complete key objects.
 */
export const CycleUserProperties = z.object({
  id: uuid(),
  workspace: uuid(),
  project: uuid(),
  cycle: uuid(),
  user: uuid(),
  ...prefsFields,
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type CycleUserProperties = z.infer<typeof CycleUserProperties>;

export async function getCycleUserProperties(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string
): Promise<CycleUserProperties> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/user-properties/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(CycleUserProperties, body, `GET ${path}`);
}

export async function updateCycleUserProperties(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string,
  patch: PrefsPatch
): Promise<CycleUserProperties> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/user-properties/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: patch });
  return client.parse(CycleUserProperties, envelope.data, `PATCH ${path}`);
}

/** Module display properties for the caller: same semantics as cycle. */
export const ModuleUserProperties = z.object({
  id: uuid(),
  workspace: uuid(),
  project: uuid(),
  module: uuid(),
  user: uuid(),
  ...prefsFields,
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type ModuleUserProperties = z.infer<typeof ModuleUserProperties>;

export async function getModuleUserProperties(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string
): Promise<ModuleUserProperties> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/modules/${moduleId}/user-properties/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ModuleUserProperties, body, `GET ${path}`);
}

export async function updateModuleUserProperties(
  client: ApiClient,
  slug: string,
  projectId: string,
  moduleId: string,
  patch: PrefsPatch
): Promise<ModuleUserProperties> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/modules/${moduleId}/user-properties/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: patch });
  return client.parse(ModuleUserProperties, envelope.data, `PATCH ${path}`);
}

/**
 * Cloud-only: epics display properties. No OSS route exists; the shape
 * mirrors the cycle/module records (same four filter keys over the
 * project base fields). Proven by stub unit tests, not live.
 */
export const EpicUserProperties = z.object({
  id: uuid(),
  workspace: uuid(),
  project: uuid(),
  user: uuid(),
  ...prefsFields,
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type EpicUserProperties = z.infer<typeof EpicUserProperties>;

export async function getEpicUserProperties(
  client: ApiClient,
  slug: string,
  projectId: string
): Promise<EpicUserProperties> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/epics-user-properties/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(EpicUserProperties, body, `GET ${path}`);
}

export async function updateEpicUserProperties(
  client: ApiClient,
  slug: string,
  projectId: string,
  patch: PrefsPatch
): Promise<EpicUserProperties> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/epics-user-properties/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: patch });
  return client.parse(EpicUserProperties, envelope.data, `PATCH ${path}`);
}

/**
 * Project display preferences for the caller: same PATCH semantics as
 * cycle/module (sent keys replaced wholesale, others untouched), so
 * callers send complete key objects. Reads via getProjectUserProperties
 * (projects.ts); the tab-preference PATCH beside it only covers
 * sort_order/preferences.
 */
export async function updateProjectDisplayPreferences(
  client: ApiClient,
  slug: string,
  projectId: string,
  patch: PrefsPatch
): Promise<ProjectUserProperties> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/user-properties/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: patch });
  return client.parse(ProjectUserProperties, envelope.data, `PATCH ${path}`);
}

/**
 * Workspace display preferences for the caller (the global all-issues
 * scope): same PATCH semantics as the project record. Reads via
 * getWorkspaceUserProperties (projects.ts).
 */
export async function updateWorkspaceDisplayPreferences(
  client: ApiClient,
  slug: string,
  patch: PrefsPatch
): Promise<WorkspaceUserProperties> {
  const path = `/api/workspaces/${slug}/user-properties/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: patch });
  return client.parse(WorkspaceUserProperties, envelope.data, `PATCH ${path}`);
}
