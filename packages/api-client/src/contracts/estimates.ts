// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project estimate contracts: the read the shell's Change-estimate picker
// uses, plus the writes the settings estimates entry needs (create the
// project's active system with its points, delete a system). The full
// estimates area owns the richer editing surface; this module is the
// shared contract both surfaces build on.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableUuid, uuid } from "./fields.js";

/** Estimate system families the picker formats (points and categories show raw values). */
export const ESTIMATE_SYSTEM_TIME = "time";
export const ESTIMATE_TYPES = ["categories", "points", "time"] as const;

export const EstimatePoint = z.object({
  id: uuid(),
  key: z.number(),
  value: z.string(),
  description: z.string(),
});
export type EstimatePoint = z.infer<typeof EstimatePoint>;

export const Estimate = z.object({
  id: uuid(),
  workspace: uuid(),
  project: uuid(),
  name: z.string(),
  description: z.string(),
  type: z.string(),
  last_used: z.boolean(),
  points: z.array(EstimatePoint),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
});
export type Estimate = z.infer<typeof Estimate>;

export const EstimateList = z.array(Estimate);
export type EstimateList = z.infer<typeof EstimateList>;

export async function listProjectEstimates(client: ApiClient, slug: string, projectId: string): Promise<EstimateList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/estimates/`);
  return client.parse(EstimateList, body, `GET estimates ${projectId}`);
}

/** The estimate system whose points the picker offers, if the project has one. */
export function selectActiveEstimate(estimates: EstimateList): Estimate | undefined {
  return estimates.find((estimate) => estimate.last_used);
}

export interface EstimatePointInput {
  key: number;
  value: string;
  description?: string;
}

export interface EstimateCreateInput {
  name: string;
  /** `categories` (default), `points` or `time`. */
  type?: string;
  /** Marks the created system as the project's active one. */
  last_used?: boolean;
  points?: EstimatePointInput[];
}

/**
 * POST .../projects/<pid>/estimates/ answers 200 (not 201) with the created
 * system including its points. The body wraps the system under `estimate`
 * and the point rows under `estimate_points`.
 */
export async function createEstimate(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: EstimateCreateInput
): Promise<Estimate> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/estimates/`;
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: {
      estimate: {
        name: input.name,
        ...(input.type === undefined ? {} : { type: input.type }),
        ...(input.last_used === undefined ? {} : { last_used: input.last_used }),
      },
      estimate_points: input.points ?? [],
    },
  });
  return client.parse(Estimate, envelope.data, `POST ${path}`);
}

/** DELETE .../estimates/<eid>/ answers 204 and drops the system's points. */
export async function deleteEstimate(
  client: ApiClient,
  slug: string,
  projectId: string,
  estimateId: string
): Promise<number> {
  const response = await client.delete(`/api/workspaces/${slug}/projects/${projectId}/estimates/${estimateId}/`);
  return response.status;
}
