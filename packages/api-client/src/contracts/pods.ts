// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pod contracts: the project-scoped runner pools behind the overview
// tiles, the create/rename dialogs, and the default-pod flag that decides
// where fresh issue runs land.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableUuid, uuid } from "./fields.js";

/** Slim pod projection nested inside runner and run rows. */
export const PodMini = z.object({
  id: uuid(),
  name: z.string(),
  is_default: z.boolean(),
  project: uuid(),
  project_identifier: z.string(),
});
export type PodMini = z.infer<typeof PodMini>;

/**
 * One pod row. The name always carries the project-identifier prefix;
 * the server adds it when a bare suffix is sent. `runner_count` counts
 * every runner on the pod, including revoked ones.
 */
export const Pod = z.object({
  id: uuid(),
  name: z.string(),
  description: z.string(),
  is_default: z.boolean(),
  workspace: uuid(),
  project: uuid(),
  project_identifier: z.string(),
  created_by: nullableUuid(),
  runner_count: z.number(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type Pod = z.infer<typeof Pod>;

export const PodList = z.array(Pod);
export type PodList = z.infer<typeof PodList>;

/**
 * List scope. The project filter wins when both are given; at least one
 * is required, otherwise the server answers 400.
 */
export interface PodListScope {
  projectId?: string;
  workspaceId?: string;
}

/**
 * Pods for a project, or the workspace-wide aggregate. Answers
 * default-first, oldest-first within each group.
 */
export async function listPods(client: ApiClient, scope: PodListScope = {}): Promise<PodList> {
  const path = "/api/runners/pods/";
  const body = await client.getJson<unknown>(path, {
    query: { project: scope.projectId, workspace: scope.workspaceId },
  });
  return client.parse(PodList, body, `GET ${path}`);
}

/** One pod row by id. */
export async function getPod(client: ApiClient, podId: string): Promise<Pod> {
  const path = `/api/runners/pods/${podId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(Pod, body, `GET ${path}`);
}

export interface PodCreateInput {
  /** Owning project id. The workspace is derived from it server-side. */
  project: string;
  /** Bare suffix or fully prefixed name; the server prefixes when missing. */
  name: string;
  description?: string;
}

/** Create a pod. Fresh pods are never the default. Answers 201. */
export async function createPod(client: ApiClient, input: PodCreateInput): Promise<Pod> {
  const path = "/api/runners/pods/";
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: { project: input.project, name: input.name, description: input.description },
  });
  return client.parse(Pod, envelope.data, `POST ${path}`);
}

export interface PodPatch {
  name?: string;
  description?: string;
  /** Promote to the project default; promoting clears the previous default. */
  is_default?: boolean;
}

/** Rename, re-describe, or promote a pod. Answers the updated row. */
export async function updatePod(client: ApiClient, podId: string, patch: PodPatch): Promise<Pod> {
  const path = `/api/runners/pods/${podId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: patch });
  return client.parse(Pod, envelope.data, `PATCH ${path}`);
}

/**
 * Soft-delete a pod; assigned issues are unassigned, runners and past
 * runs stay. Refused with 409 while runners, active runs, or the
 * default flag remain. Answers 204 with no body.
 */
export async function deletePod(client: ApiClient, podId: string): Promise<number> {
  const response = await client.delete(`/api/runners/pods/${podId}/`);
  return response.status;
}
