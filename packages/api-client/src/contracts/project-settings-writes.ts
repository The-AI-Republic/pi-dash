// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-settings write contracts: membership add/role/remove and the
// workflow-state create/update/delete/mark-default calls. These run from
// the settings tabs, so they stay out of the initial bundle; the label
// writers already live with the issues area in reference.ts.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { ProjectLite } from "./projects.js";
import { State } from "./reference.js";
import { UserLite } from "./users.js";
import { WorkspaceLite } from "./workspaces.js";

/** One row in the bulk add payload: user id plus the project role to grant. */
export interface ProjectMemberAddInput {
  member_id: string;
  role: number;
}

/**
 * A member row as the bulk-add endpoint answers it: membership id, granted
 * role, the user id and project id, the pre-write role and the joining
 * timestamp. The list read answers this same projection.
 */
export const ProjectMemberRole = z.object({
  id: uuid(),
  role: z.number(),
  member: uuid(),
  project: uuid(),
  original_role: z.number(),
  created_at: dateTime(),
});
export type ProjectMemberRole = z.infer<typeof ProjectMemberRole>;

export const ProjectMemberRoleList = z.array(ProjectMemberRole);
export type ProjectMemberRoleList = z.infer<typeof ProjectMemberRoleList>;

/**
 * PATCH answers the admin-lens serializer: the membership row with nested
 * user, workspace and project. The audit fields ride along because the
 * serializer exposes the whole model.
 */
export const ProjectMemberAdmin = z.object({
  id: uuid(),
  member: UserLite,
  workspace: WorkspaceLite,
  project: ProjectLite,
  role: z.number(),
  comment: z.nullable(z.string()),
  view_props: z.record(z.string(), z.unknown()),
  default_props: z.record(z.string(), z.unknown()),
  preferences: z.record(z.string(), z.unknown()),
  sort_order: z.number(),
  is_active: z.boolean(),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  deleted_at: nullableDateTime(),
});
export type ProjectMemberAdmin = z.infer<typeof ProjectMemberAdmin>;

/**
 * POST .../members/ bulk-adds each pair, re-activating an inactive row with
 * the requested role. The server refuses an empty list and roles outside
 * the user's workspace role. Answers 201 with every matching membership,
 * not just the rows created in this call.
 */
export async function addProjectMembers(
  client: ApiClient,
  slug: string,
  projectId: string,
  members: ProjectMemberAddInput[]
): Promise<ProjectMemberRoleList> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/members/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { members } });
  return client.parse(ProjectMemberRoleList, envelope.data, `POST ${path}`);
}

/** PATCH .../members/<id>/ changes the project role; answers the admin row. */
export async function updateProjectMember(
  client: ApiClient,
  slug: string,
  projectId: string,
  memberId: string,
  role: number
): Promise<ProjectMemberAdmin> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/members/${memberId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: { role } });
  return client.parse(ProjectMemberAdmin, envelope.data, `PATCH ${path}`);
}

/**
 * DELETE .../members/<id>/ soft-deactivates the membership (204). The row
 * disappears from the member list but still resolves through the admin
 * lens on a direct read.
 */
export async function removeProjectMember(
  client: ApiClient,
  slug: string,
  projectId: string,
  memberId: string
): Promise<number> {
  const response = await client.delete(`/api/workspaces/${slug}/projects/${projectId}/members/${memberId}/`);
  return response.status;
}

/** Fields the project-settings state form and drag handlers write. */
export interface StateCreateInput {
  name: string;
  color: string;
  group: string;
  description?: string;
  sequence?: number;
}

export interface StateUpdateInput {
  name?: string;
  color?: string;
  group?: string;
  description?: string;
  sequence?: number;
}

/**
 * POST .../states/ creates one state and answers 200 with the state row
 * (note: create answers 200 here, not 201). The server refuses the triage
 * group and duplicate names.
 */
export async function createState(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: StateCreateInput
): Promise<State> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/states/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(State, envelope.data, `POST ${path}`);
}

/**
 * PATCH .../states/<id>/ handles rename, description, color, group and
 * ordering. Ordering is the `sequence` field; the list annotates `order`
 * as the position share within the group.
 */
export async function updateState(
  client: ApiClient,
  slug: string,
  projectId: string,
  stateId: string,
  input: StateUpdateInput
): Promise<State> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/states/${stateId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(State, envelope.data, `PATCH ${path}`);
}

/**
 * DELETE .../states/<id>/ removes an empty, non-default state (204). The
 * server refuses the default state and any state still holding work items.
 */
export async function deleteState(
  client: ApiClient,
  slug: string,
  projectId: string,
  stateId: string
): Promise<number> {
  const response = await client.delete(`/api/workspaces/${slug}/projects/${projectId}/states/${stateId}/`);
  return response.status;
}

/**
 * POST .../states/<id>/mark-default/ sets this state as the project's only
 * default (204). It sends an empty JSON object; the server clears every
 * other default flag first.
 */
export async function markStateDefault(
  client: ApiClient,
  slug: string,
  projectId: string,
  stateId: string
): Promise<number> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/states/${stateId}/mark-default/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: {} });
  return envelope.status;
}
