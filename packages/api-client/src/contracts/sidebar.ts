// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar contracts: per-user sidebar preferences (pinned rows and ordering)
// and the favorites tree (entries, folders, groups, moves) behind the
// workspace sidebar and its slide-overs.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableUuid, uuid } from "./fields.js";

/** One sidebar row's personalization: pinned state and sort position. */
export const SidebarPreferenceItem = z.object({
  is_pinned: z.boolean(),
  sort_order: z.number(),
});
export type SidebarPreferenceItem = z.infer<typeof SidebarPreferenceItem>;

/**
 * The whole sidebar map. GET answers `{key: {is_pinned, sort_order}}` and
 * backfills missing keys server-side, so every known row is always present.
 */
export const SidebarPreferences = z.record(z.string(), SidebarPreferenceItem);
export type SidebarPreferences = z.infer<typeof SidebarPreferences>;

export async function getSidebarPreferences(client: ApiClient, slug: string): Promise<SidebarPreferences> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/sidebar-preferences/`);
  return client.parse(SidebarPreferences, body, `GET /api/workspaces/${slug}/sidebar-preferences/`);
}

export interface SidebarPreferencePatch {
  key: string;
  is_pinned?: boolean;
  sort_order?: number;
}

export const SidebarPreferencesPatchResult = z.object({
  message: z.string(),
});
export type SidebarPreferencesPatchResult = z.infer<typeof SidebarPreferencesPatchResult>;

/**
 * Bulk-update pinned/ordered rows. The server applies the list item by
 * item, skipping entries without a key or with an unknown key, and answers
 * a bare acknowledgement — the caller re-reads with getSidebarPreferences.
 */
export async function updateSidebarPreferences(
  client: ApiClient,
  slug: string,
  items: SidebarPreferencePatch[]
): Promise<SidebarPreferencesPatchResult> {
  const envelope = await client.sendJson<unknown>("PATCH", `/api/workspaces/${slug}/sidebar-preferences/`, {
    json: items,
  });
  return client.parse(
    SidebarPreferencesPatchResult,
    envelope.data,
    `PATCH /api/workspaces/${slug}/sidebar-preferences/`
  );
}

/**
 * One favorites-tree node. Folders are nodes with `is_folder: true` and no
 * entity; moves and ordering are PATCHes of `parent` / `sequence`.
 * `entity_data` carries the target's lite projection (shape varies by
 * entity type) and is null when the target no longer resolves.
 */
export const Favorite = z.object({
  id: uuid(),
  entity_type: z.string(),
  entity_identifier: nullableUuid(),
  entity_data: z.unknown(),
  name: z.nullable(z.string()),
  is_folder: z.boolean(),
  sequence: z.number(),
  parent: nullableUuid(),
  workspace_id: uuid(),
  project_id: nullableUuid(),
});
export type Favorite = z.infer<typeof Favorite>;

export const FavoriteList = z.array(Favorite);
export type FavoriteList = z.infer<typeof FavoriteList>;

export async function listFavorites(client: ApiClient, slug: string): Promise<FavoriteList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/user-favorites/`);
  return client.parse(FavoriteList, body, `GET /api/workspaces/${slug}/user-favorites/`);
}

export interface FavoriteCreate {
  entity_type: string;
  entity_identifier?: string | null;
  name?: string | null;
  is_folder?: boolean;
  sequence?: number;
  parent?: string | null;
  project_id?: string | null;
  /**
   * Target projection the sidebar reads back (cycles send `{ name }`
   * so the entry titles itself; parity/specs/cycles.md §11, CYC-029).
   */
  entity_data?: Record<string, unknown> | null;
}

export async function createFavorite(client: ApiClient, slug: string, input: FavoriteCreate): Promise<Favorite> {
  const envelope = await client.sendJson<unknown>("POST", `/api/workspaces/${slug}/user-favorites/`, {
    json: input,
  });
  return client.parse(Favorite, envelope.data, `POST /api/workspaces/${slug}/user-favorites/`);
}

export interface FavoritePatch {
  entity_type?: string;
  entity_identifier?: string | null;
  name?: string | null;
  is_folder?: boolean;
  sequence?: number;
  parent?: string | null;
  project_id?: string | null;
}

export async function updateFavorite(
  client: ApiClient,
  slug: string,
  favoriteId: string,
  input: FavoritePatch
): Promise<Favorite> {
  const envelope = await client.sendJson<unknown>("PATCH", `/api/workspaces/${slug}/user-favorites/${favoriteId}/`, {
    json: input,
  });
  return client.parse(Favorite, envelope.data, `PATCH /api/workspaces/${slug}/user-favorites/${favoriteId}/`);
}

export async function deleteFavorite(client: ApiClient, slug: string, favoriteId: string): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/user-favorites/${favoriteId}/`);
}

export async function listFavoriteGroup(client: ApiClient, slug: string, favoriteId: string): Promise<FavoriteList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/user-favorites/${favoriteId}/group/`);
  return client.parse(FavoriteList, body, `GET /api/workspaces/${slug}/user-favorites/${favoriteId}/group/`);
}
