// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimate system contracts (parity/specs/estimates.md §7.1). A project
// holds several measurement systems; each carries ordered values (points)
// and one system per project is flagged in use. The settings screen lists
// them, creates and deletes systems, links one to the project, and reads a
// single system for editing; pickers and the shell read the same list and
// pick the in-use system out of it. Ordering is never taken from the wire:
// nested values arrive in the model's text order, so every reader that
// renders values sorts by `key` through this module's selector.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableUuid, uuid } from "./fields.js";

/** Estimate system families the picker formats (points and categories show raw values). */
export const ESTIMATE_SYSTEM_TIME = "time";
export const ESTIMATE_TYPES = ["categories", "points", "time"] as const;

/**
 * One stored value of a system (the API calls it an estimate point). `key`
 * is the integer display position the caller assigned at creation; the
 * server stores it verbatim and does not renumber on read, so two rows can
 * share a key and the create path is the only place uniqueness is kept.
 */
export const EstimatePoint = z.object({
  id: uuid(),
  key: z.number(),
  value: z.string(),
  description: z.string(),
});
export type EstimatePoint = z.infer<typeof EstimatePoint>;

/**
 * Full system row as every read answers it. `type` is one of the families
 * above, but readers keep it a plain string: a future family must render
 * with a generic treatment rather than fail the whole list.
 */
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

/** GET .../projects/<pid>/estimates/ — every system of the project, points nested. */
export async function listProjectEstimates(client: ApiClient, slug: string, projectId: string): Promise<EstimateList> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/estimates/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(EstimateList, body, `GET ${path}`);
}

/**
 * GET .../estimates/<eid>/ — one system with its points, scoped to the
 * project in the path. An id from another project (or none at all) is a
 * 404 with `{"error": "The required object does not exist."}`.
 */
export async function getProjectEstimate(
  client: ApiClient,
  slug: string,
  projectId: string,
  estimateId: string
): Promise<Estimate> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/estimates/${estimateId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(Estimate, body, `GET ${path}`);
}

/** The estimate system whose values the project currently offers, if any. */
export function selectActiveEstimate(estimates: EstimateList): Estimate | undefined {
  return estimates.find((estimate) => estimate.last_used);
}

/** Systems no longer in use, most recently created first (the archived section). */
export function selectArchivedEstimates(estimates: EstimateList): Estimate[] {
  return estimates
    .filter((estimate) => !estimate.last_used)
    .sort((left, right) => {
      const difference = Date.parse(right.created_at) - Date.parse(left.created_at);
      return difference === 0 ? left.id.localeCompare(right.id) : difference;
    });
}

/**
 * One value's display order: ascending `key`. Ties keep list order (the
 * server never promises distinct keys for rows it did not create), so the
 * sort is stable rather than an error. Returns a new array.
 */
export function orderEstimatePoints(points: readonly EstimatePoint[]): EstimatePoint[] {
  return [...points].sort((left, right) => left.key - right.key);
}

/**
 * One point in a create body. `key` is required and is the caller's 1-based
 * display position (1..n, so 1 is the first row); the server does not
 * assign it. Sending a row
 * without `key` makes the server store its column default `0`, which lands
 * the value before every real position — the create path must never rely
 * on that default, which is why this field is not optional.
 */
export const EstimatePointCreateInput = z.object({
  key: z.int().check(z.minimum(1)),
  value: z.string(),
  description: z.optional(z.string()),
});
export type EstimatePointCreateInput = z.infer<typeof EstimatePointCreateInput>;

/**
 * Request body for POST .../estimates/. The system object is always sent
 * (the server reads its keys without a null check); an omitted `name` is
 * generated server-side, an omitted `type` defaults to `categories`, and
 * an omitted `last_used` defaults to false.
 */
export const EstimateCreateBody = z.object({
  estimate: z.object({
    name: z.optional(z.string()),
    type: z.optional(z.string()),
    last_used: z.optional(z.boolean()),
  }),
  estimate_points: z.array(EstimatePointCreateInput),
});
export type EstimateCreateBody = z.infer<typeof EstimateCreateBody>;

export interface EstimateCreateInput {
  /** Omitted: the server generates a ten-letter lowercase name. */
  name?: string;
  /** `categories` (server default), `points` or `time`. */
  type?: string;
  /** Marks the created system as the project's active one; defaults to false. */
  last_used?: boolean;
  points?: EstimatePointCreateInput[];
}

/**
 * POST .../projects/<pid>/estimates/ answers 200 (not 201) with the created
 * system including its points. The body wraps the system under `estimate`
 * and the point rows under `estimate_points`; it is validated here so a
 * caller cannot send a row without a position.
 */
export async function createEstimate(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: EstimateCreateInput
): Promise<Estimate> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/estimates/`;
  const body: EstimateCreateBody = EstimateCreateBody.parse({
    estimate: {
      ...(input.name === undefined ? {} : { name: input.name }),
      ...(input.type === undefined ? {} : { type: input.type }),
      ...(input.last_used === undefined ? {} : { last_used: input.last_used }),
    },
    estimate_points: input.points ?? [],
  });
  const envelope = await client.sendJson<unknown>("POST", path, { json: body });
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

/**
 * GET .../projects/<pid>/project-estimates/ — the values of the system the
 * project currently links, in the server's own order (callers sort by
 * `key`). Admin or member only; a guest is refused with 403. An unlinked
 * project answers an empty array rather than 404.
 */
export async function listProjectEstimatePoints(
  client: ApiClient,
  slug: string,
  projectId: string
): Promise<EstimatePoint[]> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/project-estimates/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(z.array(EstimatePoint), body, `GET ${path}`);
}
