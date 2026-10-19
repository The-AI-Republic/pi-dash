// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved views, thin slice for the issues area (ISS-104): the filter row's
// save/update entries create and patch view records carrying the filter
// expression plus display settings, and a list rendered from a saved view
// reads that record back. Full view ownership (routes, lists, dialog
// chrome) stays in the views area NEWFRONT-78.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";

/** A saved view record, project or workspace scope. */
export const SavedView = z.object({
  id: uuid(),
  workspace: uuid(),
  project: nullableUuid(),
  name: z.string(),
  description: z.string(),
  query: z.record(z.string(), z.unknown()),
  filters: z.record(z.string(), z.unknown()),
  display_filters: z.record(z.string(), z.unknown()),
  display_properties: z.record(z.string(), z.unknown()),
  rich_filters: z.record(z.string(), z.unknown()),
  access: z.number(),
  sort_order: z.number(),
  logo_props: z.record(z.string(), z.unknown()),
  owned_by: uuid(),
  is_locked: z.boolean(),
  archived_at: nullableDateTime(),
  is_favorite: z.optional(z.nullable(z.boolean())),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type SavedView = z.infer<typeof SavedView>;

/** Create payload: the name plus the filter/display snapshot. */
export interface SavedViewCreate {
  name: string;
  description?: string;
  filters?: Record<string, unknown>;
  display_filters?: Record<string, unknown>;
  display_properties?: Record<string, unknown>;
  rich_filters?: Record<string, unknown>;
  access?: number;
  logo_props?: Record<string, unknown>;
}

export interface SavedViewPatch {
  name?: string;
  description?: string;
  filters?: Record<string, unknown>;
  display_filters?: Record<string, unknown>;
  display_properties?: Record<string, unknown>;
  rich_filters?: Record<string, unknown>;
}

/** GET .../projects/<id>/views/<viewId>/ — one project view (200). */
export async function getProjectView(
  client: ApiClient,
  slug: string,
  projectId: string,
  viewId: string
): Promise<SavedView> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/views/${viewId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(SavedView, body, `GET ${path}`);
}

/** GET /api/workspaces/<slug>/views/<viewId>/ — one workspace view (200). */
export async function getWorkspaceView(client: ApiClient, slug: string, viewId: string): Promise<SavedView> {
  const path = `/api/workspaces/${slug}/views/${viewId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(SavedView, body, `GET ${path}`);
}

/** POST /api/workspaces/<slug>/views/ — create a workspace view (201). */
export async function createWorkspaceView(client: ApiClient, slug: string, input: SavedViewCreate): Promise<SavedView> {
  const path = `/api/workspaces/${slug}/views/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(SavedView, envelope.data, `POST ${path}`);
}

/** POST .../projects/<id>/views/ — create a project view (201). */
export async function createProjectView(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: SavedViewCreate
): Promise<SavedView> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/views/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(SavedView, envelope.data, `POST ${path}`);
}

/**
 * PATCH .../views/<id>/ — update a view. Only the owner may update;
 * locked views refuse with 400.
 */
export async function updateProjectView(
  client: ApiClient,
  slug: string,
  projectId: string,
  viewId: string,
  input: SavedViewPatch
): Promise<SavedView> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/views/${viewId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(SavedView, envelope.data, `PATCH ${path}`);
}

/** PATCH /api/workspaces/<slug>/views/<id>/ — update a workspace view. */
export async function updateWorkspaceView(
  client: ApiClient,
  slug: string,
  viewId: string,
  input: SavedViewPatch
): Promise<SavedView> {
  const path = `/api/workspaces/${slug}/views/${viewId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(SavedView, envelope.data, `PATCH ${path}`);
}

/**
 * DELETE — remove a view the caller owns (204). The row is blanked in
 * place and drops out of lists; retrieve still 200s. Test cleanup only.
 */
export async function deleteProjectView(
  client: ApiClient,
  slug: string,
  projectId: string,
  viewId: string
): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/views/${viewId}/`);
}

/** DELETE — remove a workspace view the caller owns (204). */
export async function deleteWorkspaceView(client: ApiClient, slug: string, viewId: string): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/views/${viewId}/`);
}
