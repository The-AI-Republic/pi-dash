// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-settings write contracts: membership add/role/remove and the
// workflow-state create/update/delete/mark-default calls. These run from
// the settings tabs, so they stay out of the initial bundle; the label
// writers already live with the issues area in reference.ts.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { ProjectLite } from "./project-lite.js";
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
 * PATCH answers `ProjectMemberSerializer`: the membership row with nested
 * thin user, workspace and project. Unlike the workspace roster, the nested
 * user is `UserLite` — no email or last-login fields.
 */
export const ProjectMemberDetail = z.object({
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
export type ProjectMemberDetail = z.infer<typeof ProjectMemberDetail>;

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

/** PATCH .../members/<id>/ changes the project role; answers the thin detail row. */
export async function updateProjectMember(
  client: ApiClient,
  slug: string,
  projectId: string,
  memberId: string,
  role: number
): Promise<ProjectMemberDetail> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/members/${memberId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: { role } });
  return client.parse(ProjectMemberDetail, envelope.data, `PATCH ${path}`);
}

/**
 * DELETE .../members/<id>/ deactivates the membership (204). The row leaves
 * the member list, and a direct read of the membership answers 404 because
 * the retrieve path only matches active rows.
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

/** Result of a state create: the parsed row and the wire status (always 200). */
export interface StateCreateResult {
  state: State;
  status: number;
}

/**
 * POST .../states/ creates one state and answers 200 with the state row
 * (note: create answers 200 here, not 201). Any other status is a contract
 * violation and rejects, so a server-side change to 201 cannot slip through.
 * The status rides back for callers that want it. The server refuses the
 * triage group and duplicate names.
 */
export async function createState(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: StateCreateInput
): Promise<StateCreateResult> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/states/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  if (envelope.status !== 200) {
    throw new ApiError({
      status: envelope.status,
      code: "contract",
      message: `POST ${path} answered ${envelope.status}, expected 200`,
      body: envelope.data,
    });
  }
  return { state: client.parse(State, envelope.data, `POST ${path}`), status: envelope.status };
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
