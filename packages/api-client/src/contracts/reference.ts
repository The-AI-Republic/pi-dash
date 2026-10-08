// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project reference-data contracts: states, labels, members.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableUuid, uuid } from "./fields.js";

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
  created_at: z.iso.datetime(),
});
export type ProjectMember = z.infer<typeof ProjectMember>;

export const ProjectMemberList = z.array(ProjectMember);
export type ProjectMemberList = z.infer<typeof ProjectMemberList>;

export async function listMembers(client: ApiClient, slug: string, projectId: string): Promise<ProjectMemberList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/members/`);
  return client.parse(ProjectMemberList, body, "GET members");
}
