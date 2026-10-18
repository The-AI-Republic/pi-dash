// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project reference-data contracts: states, labels, members.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { isApiError } from "../errors.js";
import { dateTime, nullableUuid, uuid } from "./fields.js";

export const STATE_GROUPS = [
  "backlog",
  "unstarted",
  "started",
  "review",
  "test",
  "completed",
  "cancelled",
  "triage",
] as const;

export const State = z.object({
  id: uuid(),
  project_id: uuid(),
  workspace_id: uuid(),
  name: z.string(),
  color: z.string(),
  group: z.enum(STATE_GROUPS),
  default: z.boolean(),
  description: z.string(),
  sequence: z.number(),
  order: z.optional(z.number()),
});
export type State = z.infer<typeof State>;

export const StateList = z.array(State);
export type StateList = z.infer<typeof StateList>;

export async function listStates(client: ApiClient, slug: string, projectId: string): Promise<StateList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/states/`);
  return client.parse(StateList, body, "GET states");
}

export const Label = z.object({
  id: uuid(),
  project_id: uuid(),
  workspace_id: uuid(),
  name: z.string(),
  color: z.string(),
  parent: nullableUuid(),
  sort_order: z.number(),
});
export type Label = z.infer<typeof Label>;

export const LabelList = z.array(Label);
export type LabelList = z.infer<typeof LabelList>;

export async function listLabels(client: ApiClient, slug: string, projectId: string): Promise<LabelList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/issue-labels/`);
  return client.parse(LabelList, body, "GET issue-labels");
}

export interface LabelCreate {
  name: string;
  color: string;
  parent?: string | null;
}

/**
 * Label create for the issues area: the settings form and the picker's
 * inline admin create. Admin-only; duplicate names answer 400. Fresh
 * labels take large incremental sort orders from the server.
 */
export async function createLabel(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: LabelCreate
): Promise<Label> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issue-labels/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(Label, envelope.data, `POST ${path}`);
}

export interface LabelPatch {
  name?: string;
  color?: string;
  parent?: string | null;
  sort_order?: number;
}

/**
 * Label update: rename/recolor from the settings form, parent plus
 * fractional sort-order writes from drag group/reorder. Passing parent
 * null ungroups back to top level.
 */
export async function updateLabel(
  client: ApiClient,
  slug: string,
  projectId: string,
  labelId: string,
  input: LabelPatch
): Promise<Label> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issue-labels/${labelId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(Label, envelope.data, `PATCH ${path}`);
}

/**
 * Label delete: cascades the label off every work item carrying it.
 * Admin-only like the other label writes.
 */
export async function deleteLabel(client: ApiClient, slug: string, projectId: string, labelId: string): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/issue-labels/${labelId}/`);
}

/**
 * The project's single intake (triage) state for the intake-state
 * picker. Answers 404 when the project has no triage state.
 */
export async function getIntakeState(client: ApiClient, slug: string, projectId: string): Promise<State> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/intake-state/`);
  return client.parse(State, body, `GET intake-state ${projectId}`);
}

/**
 * Project member list row. The endpoint answers the role projection — the
 * member and project ride along as bare ids, not nested objects.
 */
export const ProjectMember = z.object({
  id: uuid(),
  role: z.number(),
  member: uuid(),
  project: uuid(),
  original_role: z.number(),
  created_at: dateTime(),
});
export type ProjectMember = z.infer<typeof ProjectMember>;

export const ProjectMemberList = z.array(ProjectMember);
export type ProjectMemberList = z.infer<typeof ProjectMemberList>;

export async function listMembers(client: ApiClient, slug: string, projectId: string): Promise<ProjectMemberList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/members/`);
  return client.parse(ProjectMemberList, body, "GET members");
}

/**
 * The caller's own membership row in one project
 * (GET .../project-members/me/). Read inline: the notification detail
 * pane asks for it per selection, so it must fetch no matter what else
 * the workspace already warmed. A caller who holds no active membership
 * gets a 404, which this reports as null.
 */
export const OwnProjectMembership = z.object({
  id: uuid(),
  role: z.number(),
});
export type OwnProjectMembership = z.infer<typeof OwnProjectMembership>;

export async function getMyProjectMembership(
  client: ApiClient,
  slug: string,
  projectId: string
): Promise<OwnProjectMembership | null> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/project-members/me/`;
  try {
    const body = await client.getJson<unknown>(path);
    return client.parse(OwnProjectMembership, body, `GET ${path}`);
  } catch (error) {
    if (isApiError(error) && error.status === 404) return null;
    throw error;
  }
}
