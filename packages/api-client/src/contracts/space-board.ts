// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public board resolution for the space area (parity/specs/space.md §5.1):
// the published board's settings (the publish payload the admin dialog
// writes), the board's display metadata, and the legacy workspace/project
// link that resolves to an anchor. Public endpoints answer 404 for an
// anchor that is not published.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";

/** Project projection embedded in the board settings. */
export const SpaceBoardProject = z.object({
  id: uuid(),
  identifier: z.string(),
  name: z.string(),
  cover_image: z.nullable(z.string()),
  cover_image_url: z.nullable(z.string()),
  logo_props: z.record(z.string(), z.unknown()),
  description: z.string(),
  is_default: z.boolean(),
});
export type SpaceBoardProject = z.infer<typeof SpaceBoardProject>;

export const SpaceBoardWorkspace = z.object({
  name: z.string(),
  slug: z.string(),
  id: uuid(),
  logo_url: z.nullable(z.string()),
});
export type SpaceBoardWorkspace = z.infer<typeof SpaceBoardWorkspace>;

/**
 * A published board's settings. `project_details` is nullable because the
 * placeholder/empty publish payload serializes none; a missing key or a
 * value that fails the project shape also settles to null so the board
 * keeps its generic-mark fallback (SPC-010 branch 4) instead of failing
 * the whole settings read. `view_props` is the loose layout map the
 * publish dialog writes (enabled layouts). The three interaction flags
 * gate the public board's controls. `is_activity_enabled` rides along
 * but no space surface reads it, so it stays optional.
 */
export const SpaceBoardSettings = z.object({
  id: uuid(),
  project_details: z.catch(z.nullable(SpaceBoardProject), null),
  workspace_detail: SpaceBoardWorkspace,
  created_at: dateTime(),
  updated_at: dateTime(),
  deleted_at: nullableDateTime(),
  created_by: z.nullable(z.string()),
  updated_by: z.nullable(z.string()),
  workspace: uuid(),
  project: uuid(),
  entity_identifier: z.nullable(z.string()),
  entity_name: z.nullable(z.string()),
  anchor: z.nullable(z.string()),
  is_comments_enabled: z.boolean(),
  is_reactions_enabled: z.boolean(),
  is_votes_enabled: z.boolean(),
  view_props: z.record(z.string(), z.boolean()),
  is_activity_enabled: z.optional(z.boolean()),
  is_disabled: z.boolean(),
  intake: nullableUuid(),
});
export type SpaceBoardSettings = z.infer<typeof SpaceBoardSettings>;

/**
 * Board display metadata. Every field the public page might not have is
 * optional; `cover_image` is surfaced but deliberately never rendered.
 */
export const SpaceBoardMeta = z.object({
  id: z.optional(uuid()),
  identifier: z.optional(z.string()),
  name: z.optional(z.string()),
  description: z.optional(z.string()),
  cover_image: z.optional(z.nullable(z.string())),
  icon_prop: z.optional(z.unknown()),
  emoji: z.optional(z.nullable(z.string())),
});
export type SpaceBoardMeta = z.infer<typeof SpaceBoardMeta>;

export async function getSpaceBoardSettings(client: ApiClient, anchor: string): Promise<SpaceBoardSettings> {
  const path = `/api/public/anchor/${encodeURIComponent(anchor)}/settings/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(SpaceBoardSettings, body, `GET ${path}`);
}

export async function getSpaceBoardMeta(client: ApiClient, anchor: string): Promise<SpaceBoardMeta> {
  const path = `/api/public/anchor/${encodeURIComponent(anchor)}/meta/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(SpaceBoardMeta, body, `GET ${path}`);
}

/**
 * Legacy public address (`/spaces/:workspaceSlug/:projectId`) resolves to
 * the board anchor. Answers the same settings object, so the caller can
 * read `anchor` and send the visitor to the canonical address.
 */
export async function resolveSpaceBoardAnchor(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string
): Promise<SpaceBoardSettings> {
  const path = `/api/public/workspaces/${encodeURIComponent(workspaceSlug)}/projects/${encodeURIComponent(projectId)}/anchor/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(SpaceBoardSettings, body, `GET ${path}`);
}
