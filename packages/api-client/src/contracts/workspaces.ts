// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace read contracts: the workspaces the user belongs to, the
// own-membership read the gates need, plus the small projection embedded
// in projects and members. The slug check, creation and member directory
// live in workspace-management.ts, out of the initial bundle
// (NEWFRONT-523, size gate).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableUuid, uuid } from "./fields.js";

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
  created_at: dateTime(),
  updated_at: dateTime(),
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
