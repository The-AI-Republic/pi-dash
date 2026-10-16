// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Thin page (document) contract for the shell: the command palette's
// contextual edits rename pages. Full page ownership stays in the pages
// area; this is the slice the palette needs to compile and the gate needs
// to prove the rows.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableDate, uuid } from "./fields.js";

export const PageUpdate = z.object({
  id: uuid(),
  name: z.string(),
  access: z.optional(z.number()),
  color: z.optional(z.string()),
  is_locked: z.optional(z.boolean()),
  archived_at: z.optional(nullableDate()),
  project_ids: z.optional(z.array(uuid())),
});
export type PageUpdate = z.infer<typeof PageUpdate>;

export interface PagePatch {
  name?: string;
  color?: string;
  is_locked?: boolean;
}

export async function updatePage(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string,
  input: PagePatch
): Promise<PageUpdate> {
  const envelope = await client.sendJson<unknown>(
    "PATCH",
    `/api/workspaces/${slug}/projects/${projectId}/pages/${pageId}/`,
    { json: input }
  );
  return client.parse(PageUpdate, envelope.data, `PATCH page ${pageId}`);
}

/** Page visibility values behind Make public / Make private. */
export const PAGE_ACCESS_PUBLIC = 0;
export const PAGE_ACCESS_PRIVATE = 1;

/**
 * Thin page lock for the shell: the palette's Lock entry. Answers 204.
 * Full page ownership stays in the pages area.
 */
export async function lockPage(client: ApiClient, slug: string, projectId: string, pageId: string): Promise<void> {
  await client.post(`/api/workspaces/${slug}/projects/${projectId}/pages/${pageId}/lock/`, {});
}

/**
 * Thin page unlock for the shell: the palette's Unlock entry. Answers 204.
 * Full page ownership stays in the pages area.
 */
export async function unlockPage(client: ApiClient, slug: string, projectId: string, pageId: string): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/pages/${pageId}/lock/`);
}

/**
 * Thin page visibility for the shell: the palette's Make public / Make
 * private entry. Only the page owner may change access (otherwise 400).
 * Answers 204. Full page ownership stays in the pages area.
 */
export async function setPageAccess(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string,
  access: number
): Promise<void> {
  await client.post(`/api/workspaces/${slug}/projects/${projectId}/pages/${pageId}/access/`, {
    json: { access },
  });
}

export const PageArchiveResult = z.object({
  archived_at: z.string(),
});
export type PageArchiveResult = z.infer<typeof PageArchiveResult>;

/**
 * Thin page archive for the shell: the palette's Archive entry. Only
 * the page owner or a project admin may archive (otherwise 400); the
 * call also drops the page's favorite rows. Answers 200 with the stamp.
 * Full page ownership stays in the pages area.
 */
export async function archivePage(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string
): Promise<PageArchiveResult> {
  const envelope = await client.sendJson<unknown>(
    "POST",
    `/api/workspaces/${slug}/projects/${projectId}/pages/${pageId}/archive/`,
    {}
  );
  return client.parse(PageArchiveResult, envelope.data, `POST page ${pageId} archive`);
}

/**
 * Thin page restore for the shell: the palette's Restore entry.
 * Answers 204. Full page ownership stays in the pages area.
 */
export async function restorePage(client: ApiClient, slug: string, projectId: string, pageId: string): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/pages/${pageId}/archive/`);
}

/**
 * Thin page favorite for the shell. Pages favorite through their own
 * endpoints (backed by user-favorites with entity_type page), not the
 * generic favorites write. Both calls answer 204. Full page ownership
 * stays in the pages area.
 */
export async function addPageFavorite(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string
): Promise<void> {
  await client.post(`/api/workspaces/${slug}/projects/${projectId}/favorite-pages/${pageId}/`, {});
}

export async function removePageFavorite(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string
): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/favorite-pages/${pageId}/`);
}
