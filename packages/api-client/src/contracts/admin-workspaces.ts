// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Instance-admin workspace contracts: the god-mode list, slug probe and
// create. Ownership is server-assigned to the creating admin; the create
// body carries no owner field.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableUuid, uuid } from "./fields.js";

/** Nested owner projection on admin workspace rows. */
export const AdminWorkspaceOwner = z.object({
  id: uuid(),
  email: z.email(),
  first_name: z.string(),
  last_name: z.string(),
});
export type AdminWorkspaceOwner = z.infer<typeof AdminWorkspaceOwner>;

/**
 * One workspace row as GET /api/instances/workspaces/ reports it. Count
 * annotations arrive null when the subquery finds no rows; the create
 * response omits them (no annotation on a single save), so both stay
 * optional here and callers treat absence like null.
 */
export const AdminWorkspace = z.object({
  id: uuid(),
  name: z.string(),
  slug: z.string(),
  logo: z.nullable(z.string()),
  logo_asset: nullableUuid(),
  logo_url: z.nullable(z.string()),
  owner: z.nullable(AdminWorkspaceOwner),
  organization_size: z.nullable(z.string()),
  timezone: z.string(),
  background_color: z.string(),
  total_projects: z.optional(z.nullable(z.number())),
  total_members: z.optional(z.nullable(z.number())),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type AdminWorkspace = z.infer<typeof AdminWorkspace>;

/** Cursor-paginated envelope for the admin workspace list (ten per page). */
export const AdminWorkspaceListResponse = z.object({
  grouped_by: z.nullable(z.string()),
  sub_grouped_by: z.nullable(z.string()),
  total_count: z.number(),
  next_cursor: z.string(),
  prev_cursor: z.string(),
  next_page_results: z.boolean(),
  prev_page_results: z.boolean(),
  count: z.number(),
  total_results: z.number(),
  results: z.array(AdminWorkspace),
});
export type AdminWorkspaceListResponse = z.infer<typeof AdminWorkspaceListResponse>;

export async function listInstanceWorkspaces(client: ApiClient, cursor?: string): Promise<AdminWorkspaceListResponse> {
  const path = "/api/instances/workspaces/";
  const body = await client.getJson<unknown>(path, {
    query: { cursor: cursor === undefined || cursor === "" ? undefined : cursor },
  });
  return client.parse(AdminWorkspaceListResponse, body, `GET ${path}`);
}

/**
 * Slug availability probe: taken and restricted slugs answer
 * `{ status: false }`. A missing slug answers 400.
 */
export const InstanceWorkspaceSlugStatus = z.object({
  status: z.boolean(),
});
export type InstanceWorkspaceSlugStatus = z.infer<typeof InstanceWorkspaceSlugStatus>;

export async function checkInstanceWorkspaceSlug(
  client: ApiClient,
  slug: string
): Promise<InstanceWorkspaceSlugStatus> {
  const path = "/api/instances/workspace-slug-check/";
  const body = await client.getJson<unknown>(path, { query: { slug } });
  return client.parse(InstanceWorkspaceSlugStatus, body, `GET ${path}`);
}

export interface InstanceWorkspaceCreateInput {
  name: string;
  slug: string;
  organization_size: string;
}

/**
 * POST /api/instances/workspaces/ creates a workspace (201) with the owner
 * assigned server-side. The body carries no owner field.
 */
export async function createInstanceWorkspace(
  client: ApiClient,
  input: InstanceWorkspaceCreateInput
): Promise<AdminWorkspace> {
  const path = "/api/instances/workspaces/";
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(AdminWorkspace, envelope.data, `POST ${path}`);
}
