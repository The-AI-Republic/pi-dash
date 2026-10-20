// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved views (parity/specs/views.md §2, NEWFRONT-718): the create/patch
// thin slice the issues filter row uses (ISS-104) plus the list and
// retrieve reads both view scopes open with. The record is one shape at
// two scopes; the server is its authority. A retrieve that finds no
// record answers 200 with a blank serialized row instead of 404 (bug
// NEWFRONT-227), so the read helpers turn that body into the same 404
// error a genuinely missing id would raise; a screen treats every
// missing/forbidden read as its missing-view state (spec §7).
import * as z from "zod/mini";
import { ApiClient, type RequestOptions } from "../client.js";
import { ApiError } from "../errors.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { createFavorite, deleteFavorite, type Favorite } from "./sidebar.js";

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
  // Cloud edition only (VIEW-020): the public read-only anchor the publish
  // flow writes. The OSS record is created and read without it.
  anchor: z.optional(z.nullable(z.string())),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type SavedView = z.infer<typeof SavedView>;

export const SavedViewList = z.array(SavedView);
export type SavedViewList = z.infer<typeof SavedViewList>;

/** Query keys the view list endpoints accept. */
export interface SavedViewListQuery {
  /** Comma list; the server currently answers the full record regardless. */
  fields?: string;
  /** Workspace list only: `-created_at` is its default. */
  orderBy?: string;
}

/**
 * GET .../projects/<id>/views/ — the caller's visible project views. The
 * server annotates `is_favorite` and orders favorites first, then by name;
 * a guest without `guest_view_all_features` sees only its own rows.
 */
export async function listProjectViews(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: SavedViewListQuery = {}
): Promise<SavedViewList> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/views/`;
  const body = await client.getJson<unknown>(path, { query: viewListQueryParams(query) });
  return client.parse(SavedViewList, body, `GET ${path}`);
}

/**
 * GET /api/workspaces/<slug>/views/ — the workspace's custom views (the
 * four static defaults are client-side and have no record). A workspace
 * guest sees only its own rows.
 */
export async function listWorkspaceViews(
  client: ApiClient,
  slug: string,
  query: SavedViewListQuery = {}
): Promise<SavedViewList> {
  const path = `/api/workspaces/${slug}/views/`;
  const body = await client.getJson<unknown>(path, { query: viewListQueryParams(query) });
  return client.parse(SavedViewList, body, `GET ${path}`);
}

function viewListQueryParams(query: SavedViewListQuery): NonNullable<RequestOptions["query"]> {
  const params: NonNullable<RequestOptions["query"]> = {};
  if (query.fields !== undefined) params["fields"] = query.fields;
  if (query.orderBy !== undefined) params["order_by"] = query.orderBy;
  return params;
}

/**
 * A retrieve body that carries no `id`, or only a blank name, is the
 * server's stand-in for a missing record — not a view. Reading it as one
 * would render a nameless phantom (NEWFRONT-227), so it becomes the 404
 * the endpoint should have answered.
 */
function viewFromRetrieve(client: ApiClient, body: unknown, path: string): SavedView {
  const missing =
    body === null ||
    typeof body !== "object" ||
    Array.isArray(body) ||
    (body as { id?: unknown; name?: unknown }).id == null ||
    (body as { name?: unknown }).name === "";
  if (missing) {
    throw new ApiError({ status: 404, code: "not_found", message: `GET ${path}: view not found` });
  }
  return client.parse(SavedView, body, `GET ${path}`);
}

/** Create payload: the name plus the filter/display snapshot. */
/**
 * Create payload: the dialog's title and description plus the filter and
 * display snapshot. `access` and `is_locked` are serializer read-only, so
 * they are deliberately absent — a typed selector here would build a
 * control whose writes the server drops (spec §1).
 */
export interface SavedViewCreate {
  name: string;
  description?: string;
  filters?: Record<string, unknown>;
  display_filters?: Record<string, unknown>;
  display_properties?: Record<string, unknown>;
  rich_filters?: Record<string, unknown>;
  logo_props?: Record<string, unknown>;
}

/** Patch payload: any subset of the create fields; `query` is recomputed. */
export interface SavedViewPatch {
  name?: string;
  description?: string;
  filters?: Record<string, unknown>;
  display_filters?: Record<string, unknown>;
  display_properties?: Record<string, unknown>;
  rich_filters?: Record<string, unknown>;
  logo_props?: Record<string, unknown>;
}

/**
 * GET .../projects/<id>/views/<viewId>/ — one project view. An unknown id
 * answers 200 with the blank record and is reported as a 404 ApiError. A
 * hidden shared view answers 403 and a hidden owner-only row can answer
 * 500; the caller treats every such failure as its missing-view state
 * (spec §2, §7).
 */
export async function getProjectView(
  client: ApiClient,
  slug: string,
  projectId: string,
  viewId: string
): Promise<SavedView> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/views/${viewId}/`;
  const body = await client.getJson<unknown>(path);
  return viewFromRetrieve(client, body, path);
}

/**
 * GET /api/workspaces/<slug>/views/<viewId>/ — one workspace custom. The
 * retrieve is not guest-narrowed; the blank record still means missing,
 * and the screen narrows visibility against the list it read (spec §2).
 */
export async function getWorkspaceView(client: ApiClient, slug: string, viewId: string): Promise<SavedView> {
  const path = `/api/workspaces/${slug}/views/${viewId}/`;
  const body = await client.getJson<unknown>(path);
  return viewFromRetrieve(client, body, path);
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

/**
 * The favorite row a view's star reads and removes. Views use the generic
 * favorites collection rather than the legacy view pair (spec §2); the
 * add is idempotent and answers the existing row on a repeat. The server
 * fills `entity_data` from the view it points at, so the body carries only
 * the pointer and the display name.
 */
export async function addViewFavorite(
  client: ApiClient,
  slug: string,
  view: Pick<SavedView, "id" | "name" | "project" | "logo_props">
): Promise<Favorite> {
  return createFavorite(client, slug, {
    entity_type: "view",
    entity_identifier: view.id,
    project_id: view.project,
    name: view.name,
  });
}

/**
 * Remove a view favorite by the favorite row's own id — not the view id.
 * The generic workspace list omits view rows, so take the id from the
 * add response (spec §2).
 */
export async function removeViewFavorite(client: ApiClient, slug: string, favoriteId: string): Promise<void> {
  await deleteFavorite(client, slug, favoriteId);
}
