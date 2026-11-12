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
 * The member projection the directory answers for workspace members and
 * admins: the shared user-lite fields plus the account email. The thin
 * lens a workspace guest receives omits the extras, so `email` stays
 * optional and readers render a blank cell rather than rejecting the
 * whole list.
 */
export const DirectoryMember = z.extend(UserLite, {
  email: z.optional(z.string()),
});
export type DirectoryMember = z.infer<typeof DirectoryMember>;

/**
 * One workspace membership with the nested member projection. The list
 * also carries deactivated memberships; the member picker renders those
 * suspended (parity/specs/issues.md §17, ISS-208). The directory also
 * carries the admin lens' extra fields (last login); only the fields the
 * readers use are parsed.
 */
export const WorkspaceMember = z.object({
  id: uuid(),
  member: DirectoryMember,
  role: z.number(),
  /** False on deactivated memberships. Absent where the lens omits it. */
  is_active: z.optional(z.boolean()),
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

/**
 * Full workspace row as GET and PATCH /api/workspaces/<slug>/ render it:
 * the stored details plus the member-count and logo-URL annotations. The
 * detail reads carry no `role` — unlike the creation echo, the retrieve
 * queryset annotates only the member count, so the caller's role comes
 * from the membership reads instead.
 */
export const WorkspaceDetails = z.object({
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
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type WorkspaceDetails = z.infer<typeof WorkspaceDetails>;

/**
 * GET /api/workspaces/<slug>/ — the stored details the settings general
 * form edits. Reads stay allowed for every workspace role; the pages deny
 * out-of-role visitors client-side instead.
 */
export async function getWorkspace(client: ApiClient, slug: string): Promise<WorkspaceDetails> {
  const path = `/api/workspaces/${slug}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(WorkspaceDetails, body, `GET ${path}`);
}

export interface WorkspaceUpdateInput {
  name?: string;
  organization_size?: string;
  timezone?: string;
  logo?: string;
}

/**
 * PATCH /api/workspaces/<slug>/ — the admin-only details write behind the
 * general form (member and guest writes answer 403) and behind the logo
 * flow, which uploads first and then stores the returned URL here. An
 * emptied logo clears it. Answers the updated details row.
 */
export async function updateWorkspace(
  client: ApiClient,
  slug: string,
  input: WorkspaceUpdateInput
): Promise<WorkspaceDetails> {
  const path = `/api/workspaces/${slug}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(WorkspaceDetails, envelope.data, `PATCH ${path}`);
}

/**
 * DELETE /api/workspaces/<slug>/ — the admin-only workspace deletion
 * behind the general page's delete section (204). Member and guest
 * deletes answer 403.
 */
export async function deleteWorkspace(client: ApiClient, slug: string): Promise<number> {
  const response = await client.delete(`/api/workspaces/${slug}/`);
  return response.status;
}

/**
 * Admin-lens member projection: the shared UserLite subset plus the email
 * and last sign-in medium the settings roster columns need. Guests answer
 * the thin lens instead (no email), but the settings pages refuse guests,
 * so the roster always parses the admin lens.
 */
export const WorkspaceRosterUser = z.object({
  id: uuid(),
  first_name: z.string(),
  last_name: z.string(),
  avatar: z.string(),
  avatar_url: z.nullable(z.string()),
  display_name: z.string(),
  is_bot: z.boolean(),
  email: z.string(),
  last_login_medium: z.string(),
});
export type WorkspaceRosterUser = z.infer<typeof WorkspaceRosterUser>;

/**
 * One settings roster row: membership id, nested admin-lens user, role,
 * active flag, and joining date. The list also returns deactivated
 * memberships, which the roster renders suspended.
 */
export const WorkspaceRosterMember = z.object({
  id: uuid(),
  member: WorkspaceRosterUser,
  role: z.number(),
  is_active: z.boolean(),
  created_at: dateTime(),
});
export type WorkspaceRosterMember = z.infer<typeof WorkspaceRosterMember>;

export const WorkspaceRosterMemberList = z.array(WorkspaceRosterMember);
export type WorkspaceRosterMemberList = z.infer<typeof WorkspaceRosterMemberList>;

/**
 * GET /api/workspaces/<slug>/members/ through the admin lens: the
 * settings roster with email, last sign-in medium, active flag, and
 * joining date. The thin listWorkspaceMembers projection stays for the
 * pickers that need only id, member, and role.
 */
export async function listWorkspaceRoster(client: ApiClient, slug: string): Promise<WorkspaceRosterMemberList> {
  const path = `/api/workspaces/${slug}/members/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(WorkspaceRosterMemberList, body, `GET ${path}`);
}

export interface WorkspaceMemberRoleInput {
  role: number;
}

/**
 * PATCH /api/workspaces/<slug>/members/<membershipId>/ — the admin-only
 * role write behind the roster's account-type column. Answers the updated
 * row. Self role-change answers 400 and member-actor writes answer 403.
 */
export async function updateWorkspaceMemberRole(
  client: ApiClient,
  slug: string,
  membershipId: string,
  input: WorkspaceMemberRoleInput
): Promise<WorkspaceMember> {
  const path = `/api/workspaces/${slug}/members/${membershipId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(WorkspaceMember, envelope.data, `PATCH ${path}`);
}

/**
 * DELETE /api/workspaces/<slug>/members/<membershipId>/ — the admin-only
 * removal behind the roster's row menu (204). Deactivates server-side;
 * the row stays listed with is_active false. Self-removal answers 400
 * (leave instead) and removing a higher role answers 400.
 */
export async function removeWorkspaceMember(client: ApiClient, slug: string, membershipId: string): Promise<number> {
  const response = await client.delete(`/api/workspaces/${slug}/members/${membershipId}/`);
  return response.status;
}

/**
 * POST /api/workspaces/<slug>/members/leave/ with no body — the self-leave
 * behind the roster's Leave entry (204). The only admin cannot leave (400),
 * and neither can a member who solely admins a project (400).
 */
export async function leaveWorkspace(client: ApiClient, slug: string): Promise<number> {
  const response = await client.post(`/api/workspaces/${slug}/members/leave/`, {});
  return response.status;
}
