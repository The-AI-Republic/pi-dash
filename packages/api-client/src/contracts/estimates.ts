// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Thin estimate contract for the shell: the command palette's Change
// estimate picker reads the project's estimate systems with their points
// and offers the active one (last_used). Full estimate ownership stays
// in the estimates area; this is the slice the palette needs to compile
// and the gate needs to prove the rows.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { uuid } from "./fields.js";

/** Estimate system families the picker formats (points and categories show raw values). */
export const ESTIMATE_SYSTEM_TIME = "time";

export const EstimatePoint = z.object({
  id: uuid(),
  key: z.number(),
  value: z.string(),
});
export type EstimatePoint = z.infer<typeof EstimatePoint>;

export const Estimate = z.object({
  id: uuid(),
  name: z.string(),
  type: z.string(),
  last_used: z.boolean(),
  points: z.array(EstimatePoint),
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
