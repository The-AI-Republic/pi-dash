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
// renders values sorts by `key` through this module's selector. The second
// half of the module is the editing surface: the saved-system update,
// single-value create/update/delete, and the history-entry family the
// estimate writes record.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableUuid, uuid } from "./fields.js";
import type { IssueActivity } from "./issue-activity.js";

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
 * on that default, which is why this field is not optional. The integer
 * range itself is enforced by `createEstimate`, not the schema: new zod
 * check APIs ride their whole module into the web initial bundle, so this
 * schema stays on the primitives the shell already ships (NEWFRONT-518,
 * size gate).
 */
export const EstimatePointCreateInput = z.object({
  key: z.number(),
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
 * caller cannot send a row without a position. Each position must be an
 * integer at least 1: fractional, zero, negative and non-numeric keys are
 * refused before anything is sent.
 */
export async function createEstimate(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: EstimateCreateInput
): Promise<Estimate> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/estimates/`;
  for (const [index, point] of (input.points ?? []).entries()) {
    if (typeof point.key !== "number" || !Number.isInteger(point.key) || point.key < 1) {
      throw new Error(`estimate point ${index} must carry its 1-based integer position`);
    }
  }
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

/**
 * One value row in a system-update body. `id` is required: the server
 * matches rows by id and silently keeps any row it does not recognise,
 * so a row without an id would look saved while nothing changed. The
 * position and text mirror the stored row; the server writes them
 * verbatim with no length or content check.
 */
export const EstimatePointUpdateRow = z.object({
  id: uuid(),
  key: z.number(),
  value: z.string(),
});
export type EstimatePointUpdateRow = z.infer<typeof EstimatePointUpdateRow>;

/**
 * Request body for PATCH .../estimates/<eid>/. The system object carries
 * only the fields the server reads from it (name, type); the value list
 * must be non-empty or the server answers 400. The call updates existing
 * rows only — added values go through the single-point create and removed
 * ones through the single-point delete (spec §4.1).
 */
export const EstimateUpdateBody = z.object({
  estimate: z.optional(
    z.object({
      name: z.optional(z.string()),
      type: z.optional(z.string()),
    })
  ),
  estimate_points: z.array(EstimatePointUpdateRow).check(z.minLength(1)),
});
export type EstimateUpdateBody = z.infer<typeof EstimateUpdateBody>;

export interface EstimateUpdateInput {
  name?: string;
  type?: string;
  /** Every surviving value with its new position and text; never empty. */
  points: EstimatePointUpdateRow[];
}

/**
 * PATCH .../estimates/<eid>/ answers 200 with the system and its points.
 * The body is validated here so a caller cannot send an empty set (the
 * server 400s it) or a row without an id (the server would 200-ignore
 * it, a silent no-op). Unknown ids are ignored the same way, so callers
 * send only ids they read back from this system.
 */
export async function updateProjectEstimate(
  client: ApiClient,
  slug: string,
  projectId: string,
  estimateId: string,
  input: EstimateUpdateInput
): Promise<Estimate> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/estimates/${estimateId}/`;
  const renamed = {
    ...(input.name === undefined ? {} : { name: input.name }),
    ...(input.type === undefined ? {} : { type: input.type }),
  };
  const body: EstimateUpdateBody = EstimateUpdateBody.parse({
    ...(Object.keys(renamed).length > 0 ? { estimate: renamed } : {}),
    estimate_points: input.points,
  });
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: body });
  return client.parse(Estimate, envelope.data, `PATCH ${path}`);
}

/**
 * Body for POST .../estimates/<eid>/estimate-points/. Both fields are
 * required: the server answers 400 when either is falsy, and a zero
 * position counts as falsy there, so the caller's position starts at 1.
 * This shape deliberately promises no value-length cap: the server
 * stores the text without the 20-character check the bulk and update
 * paths run (NEWFRONT-347), so callers cap the value themselves and
 * never rely on this endpoint to refuse it.
 */
export const EstimatePointCreateBody = z.object({
  key: z.int().check(z.minimum(1)),
  value: z.string().check(z.minLength(1)),
});
export type EstimatePointCreateBody = z.infer<typeof EstimatePointCreateBody>;

/**
 * POST .../estimates/<eid>/estimate-points/ answers 200 with the created
 * value, id included. New rows from the edit dialog are sent through
 * here after the system PATCH, in display order.
 */
export async function createProjectEstimatePoint(
  client: ApiClient,
  slug: string,
  projectId: string,
  estimateId: string,
  input: EstimatePointCreateBody
): Promise<EstimatePoint> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/estimates/${estimateId}/estimate-points/`;
  const body: EstimatePointCreateBody = EstimatePointCreateBody.parse(input);
  const envelope = await client.sendJson<unknown>("POST", path, { json: body });
  return client.parse(EstimatePoint, envelope.data, `POST ${path}`);
}

/**
 * Partial value update for PATCH .../estimate-points/<ptid>/. Either
 * field may be sent; the server runs the point serializer over what it
 * gets, so a value over 20 characters is 400 and an empty object is 400
 * too. The length cap is the server's to enforce — this shape carries
 * the text through so the refusal stays observable.
 */
export interface EstimatePointPatch {
  key?: number;
  value?: string;
}

/** PATCH .../estimate-points/<ptid>/ answers 200 with the updated value. */
export async function updateProjectEstimatePoint(
  client: ApiClient,
  slug: string,
  projectId: string,
  estimateId: string,
  pointId: string,
  input: EstimatePointPatch
): Promise<EstimatePoint> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/estimates/${estimateId}/estimate-points/${pointId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(EstimatePoint, envelope.data, `PATCH ${path}`);
}

/**
 * Single-value delete for DELETE .../estimate-points/<ptid>/.
 * `newEstimateId` names the replacement *value* (a point id, not a
 * system id): affected work items move to it, and omitting it clears
 * them. The server renumbers the survivors and answers 200 with only
 * the rows whose positions changed — not the full remaining set — so
 * callers re-read the system instead of treating the answer as the new
 * list.
 */
export interface EstimatePointDeleteInput {
  newEstimateId?: string;
}

/**
 * DELETE .../estimate-points/<ptid>/ answers 200 with the renumbered
 * rows. Present `newEstimateId` reassigns, absent clears.
 */
export async function deleteProjectEstimatePoint(
  client: ApiClient,
  slug: string,
  projectId: string,
  estimateId: string,
  pointId: string,
  input: EstimatePointDeleteInput = {}
): Promise<EstimatePoint[]> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/estimates/${estimateId}/estimate-points/${pointId}/`;
  const envelope = await client.sendJson<unknown>(
    "DELETE",
    path,
    input.newEstimateId === undefined ? {} : { json: { new_estimate_id: input.newEstimateId } }
  );
  return client.parse(z.array(EstimatePoint), envelope.data, `DELETE ${path}`);
}

/**
 * History fields the estimate worker writes (EST-023): one per value
 * family — `estimate_points`, `estimate_categories`, `estimate_time` —
 * plus the legacy `estimate_point` on older rows. The feed recognises
 * the whole family so a future family renders with the generic estimate
 * treatment instead of vanishing.
 */
export type EstimateActivityField = `estimate_${string}`;

/** Whether a history row's field belongs to the estimate family. */
export function isEstimateActivityField(field: string | null | undefined): field is EstimateActivityField {
  return typeof field === "string" && field.startsWith("estimate_");
}

/**
 * The estimate entries of a history list, in feed order. The read itself
 * stays with the issues area (`listIssueActivities`); this only picks
 * the family out of what it answered.
 */
export function selectEstimateActivities(activities: readonly IssueActivity[]): IssueActivity[] {
  return activities.filter((entry) => isEstimateActivityField(entry.field));
}
