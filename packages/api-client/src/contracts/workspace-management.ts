// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace management contracts: the slug-availability check, workspace
// creation, and the workspace member directory. The membership list and
// own-membership reads stay in workspaces.ts; everything here runs from
// lazy surfaces (onboarding, the creation dialog, the member picker), so
// it stays out of the initial bundle (NEWFRONT-523, size gate).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableUuid, uuid } from "./fields.js";
import { UserLite } from "./users.js";

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
  created_at: dateTime(),
  updated_at: dateTime(),
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
 * One active workspace membership with the nested member projection. The
 * backend answers the admin projection for privileged callers (extra email
 * and last-login fields); the shared UserLite subset below is what
 * readers need, so extras are ignored.
 */
export const WorkspaceMember = z.object({
  id: uuid(),
  member: UserLite,
  role: z.number(),
});
export type WorkspaceMember = z.infer<typeof WorkspaceMember>;

export const WorkspaceMemberList = z.array(WorkspaceMember);
export type WorkspaceMemberList = z.infer<typeof WorkspaceMemberList>;

/**
 * GET /api/workspaces/<slug>/members/ lists the workspace's active
 * memberships (id, nested member, role). All workspace roles may read it;
 * the projects list filter panel offers these as lead/member options.
 */
export async function listWorkspaceMembers(client: ApiClient, slug: string): Promise<WorkspaceMemberList> {
  const path = `/api/workspaces/${slug}/members/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(WorkspaceMemberList, body, `GET ${path}`);
}
