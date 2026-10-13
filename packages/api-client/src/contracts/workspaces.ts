// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace contracts: the workspaces the user belongs to, plus the small
// projection embedded in projects and members.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableUuid, uuid } from "./fields.js";

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

export const WorkspaceSlugStatus = z.object({
  status: z.boolean(),
});
export type WorkspaceSlugStatus = z.infer<typeof WorkspaceSlugStatus>;

/**
 * GET /api/workspace-slug-check/?slug=<slug> reports whether the slug is
 * free: taken and restricted slugs answer `{ status: false }`. A missing
 * slug answers 400.
 */
export async function checkWorkspaceSlug(client: ApiClient, slug: string): Promise<WorkspaceSlugStatus> {
  const body = await client.getJson<unknown>("/api/workspace-slug-check/", { query: { slug } });
  return client.parse(WorkspaceSlugStatus, body, "GET /api/workspace-slug-check/");
}

export interface WorkspaceCreateInput {
  name: string;
  slug: string;
  organization_size?: string;
}

/**
 * The created workspace row: the full serializer plus the membership
 * annotations (the caller is its first admin with one member).
 */
export const CreatedWorkspace = z.object({
  id: uuid(),
  name: z.string(),
  slug: z.string(),
  logo: z.nullable(z.string()),
  logo_asset: nullableUuid(),
  logo_url: z.nullable(z.string()),
  owner: uuid(),
  organization_size: z.nullable(z.string()),
  timezone: z.string(),
  background_color: z.string(),
  total_members: z.number(),
  role: z.number(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  created_at: z.iso.datetime(),
  updated_at: z.iso.datetime(),
});
export type CreatedWorkspace = z.infer<typeof CreatedWorkspace>;

/**
 * POST /api/workspaces/ creates a workspace (201). A taken, restricted or
 * malformed slug answers 400 with a field error.
 */
export async function createWorkspace(client: ApiClient, input: WorkspaceCreateInput): Promise<CreatedWorkspace> {
  const path = "/api/workspaces/";
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(CreatedWorkspace, envelope.data, `POST ${path}`);
}

/**
 * The caller's own membership in one workspace, with the draft count
 * annotated. The denial screens read this to tell "not a member" from
 * "workspace does not exist".
 */
export const OwnWorkspaceMembership = z.object({
  id: uuid(),
  workspace: uuid(),
  member: uuid(),
  role: z.number(),
  company_role: z.nullable(z.string()),
  view_props: z.record(z.string(), z.unknown()),
  default_props: z.record(z.string(), z.unknown()),
  issue_props: z.record(z.string(), z.unknown()),
  is_active: z.boolean(),
  getting_started_checklist: z.record(z.string(), z.unknown()),
  tips: z.record(z.string(), z.unknown()),
  explored_features: z.record(z.string(), z.unknown()),
  draft_issue_count: z.number(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  created_at: z.iso.datetime(),
  updated_at: z.iso.datetime(),
});
export type OwnWorkspaceMembership = z.infer<typeof OwnWorkspaceMembership>;

/**
 * GET /api/workspaces/<slug>/workspace-members/me/. Non-members get a
 * 200 null-pattern row instead of a 404; this reports that as null.
 */
export async function getMyWorkspaceMembership(
  client: ApiClient,
  slug: string
): Promise<OwnWorkspaceMembership | null> {
  const path = `/api/workspaces/${slug}/workspace-members/me/`;
  const body = await client.getJson<unknown>(path);
  if (body === null || typeof body !== "object" || (body as { member?: unknown }).member == null) return null;
  return client.parse(OwnWorkspaceMembership, body, `GET ${path}`);
}
