// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Thin cycle contract for the shell: the command palette's contextual edits
// rename and reschedule cycles. Full cycle ownership stays in the cycles
// area; this is the slice the palette needs to compile and the gate needs
// to prove the rows. PATCH applies the edit and answers the updated
// representation.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, uuid } from "./fields.js";

export const CycleUpdate = z.object({
  id: uuid(),
  name: z.string(),
  project_id: uuid(),
  description: z.optional(z.string()),
  start_date: nullableDateTime(),
  end_date: nullableDateTime(),
});
export type CycleUpdate = z.infer<typeof CycleUpdate>;

/**
 * Thin cycle row for the shell and the issues cycle picker: the palette's
 * Add-to-cycle picker lists names while the dropdown additionally filters
 * on status. The server already excludes archived cycles. Full cycle
 * ownership stays in the cycles area.
 */
export const CycleListItem = z.object({
  id: uuid(),
  project_id: uuid(),
  name: z.string(),
  is_favorite: z.boolean(),
  // Lifecycle marker the cycle picker filters on (completed cycles are
  // excluded). The server sends it uppercase (DRAFT, ACTIVE, ...), so
  // readers compare case-insensitively. Optional: rows already held by
  // existing readers omit it, and those readers must keep parsing.
  status: z.optional(z.string()),
});
export type CycleListItem = z.infer<typeof CycleListItem>;

export const CycleList = z.array(CycleListItem);
export type CycleList = z.infer<typeof CycleList>;

export async function listCycles(client: ApiClient, slug: string, projectId: string): Promise<CycleList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/cycles/`);
  return client.parse(CycleList, body, `GET cycles ${projectId}`);
}

/**
 * The progress counts the completed-cycle transfer prompt reads (ISS-224).
 * The cycle list and detail rows carry no per-group breakdown, so the
 * counts come from the progress endpoint. Once a transfer runs, the
 * server freezes the pre-move counts into the cycle's progress snapshot
 * and this endpoint serves the frozen numbers from then on, which is why
 * the source keeps its counts and the prompt stays up.
 */
export const CycleProgress = z.object({
  total_issues: z.number(),
  backlog_issues: z.number(),
  unstarted_issues: z.number(),
  started_issues: z.number(),
});
export type CycleProgress = z.infer<typeof CycleProgress>;

/** GET .../cycles/<id>/progress/: the cycle's per-group issue counts. */
export async function getCycleProgress(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string
): Promise<CycleProgress> {
  const body = await client.getJson<unknown>(
    `/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/progress/`
  );
  return client.parse(CycleProgress, body, `GET cycle progress ${cycleId}`);
}

export interface CyclePatch {
  name?: string;
  description?: string;
  start_date?: string | null;
  end_date?: string | null;
}

export async function updateCycle(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string,
  input: CyclePatch
): Promise<CycleUpdate> {
  const envelope = await client.sendJson<unknown>(
    "PATCH",
    `/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/`,
    { json: input }
  );
  return client.parse(CycleUpdate, envelope.data, `PATCH cycle ${cycleId}`);
}

/** POST .../cycles/<id>/transfer-issues/ answers 200 {message}. */
export const CycleTransferResult = z.object({
  message: z.string(),
});
export type CycleTransferResult = z.infer<typeof CycleTransferResult>;

/**
 * Move a cycle's remaining issues into another cycle (ISS-224). The
 * transfer freezes the source progress snapshot, so the source keeps
 * its counts by design. Issues-area call: the transfer prompt lives on
 * the issues surfaces; the cycles area owns the cycle pages.
 */
export async function transferCycleIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string,
  newCycleId: string
): Promise<CycleTransferResult> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/transfer-issues/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { new_cycle_id: newCycleId } });
  return client.parse(CycleTransferResult, envelope.data, `POST ${path}`);
}

/**
 * One row of the archived-cycles list (ARCH-014). A flat array, newest
 * created first within favorites; the status is server-derived from the
 * dates (CURRENT, UPCOMING, COMPLETED, DRAFT). The backend selects its
 * columns with .values(), so every key below is always present.
 */
export const ArchivedCycleListItem = z.object({
  id: uuid(),
  workspace_id: uuid(),
  project_id: uuid(),
  name: z.string(),
  description: z.string(),
  start_date: nullableDateTime(),
  end_date: nullableDateTime(),
  owned_by_id: uuid(),
  status: z.string(),
  archived_at: dateTime(),
  is_favorite: z.boolean(),
  total_issues: z.number(),
  completed_issues: z.number(),
  started_issues: z.number(),
  unstarted_issues: z.number(),
  cancelled_issues: z.number(),
  backlog_issues: z.number(),
  assignee_ids: z.array(uuid()),
});
export type ArchivedCycleListItem = z.infer<typeof ArchivedCycleListItem>;

export const ArchivedCycleList = z.array(ArchivedCycleListItem);
export type ArchivedCycleList = z.infer<typeof ArchivedCycleList>;

/** GET .../archived-cycles/: the project's archived cycles, flat. */
export async function listArchivedCycles(
  client: ApiClient,
  slug: string,
  projectId: string
): Promise<ArchivedCycleList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/archived-cycles/`);
  return client.parse(ArchivedCycleList, body, `GET archived cycles ${projectId}`);
}

/**
 * Assignee/label distribution envelope the archived detail reads carry.
 * Rows are string-keyed dicts (dimension columns plus count or estimate
 * sums); the estimate variant is an empty object when the project has
 * no points estimate, so every member is optional there.
 */
export const ArchivedDistribution = z.object({
  assignees: z.array(z.record(z.string(), z.unknown())),
  labels: z.array(z.record(z.string(), z.unknown())),
  completion_chart: z.unknown(),
});
export type ArchivedDistribution = z.infer<typeof ArchivedDistribution>;

export const ArchivedEstimateDistribution = z.object({
  assignees: z.optional(z.array(z.record(z.string(), z.unknown()))),
  labels: z.optional(z.array(z.record(z.string(), z.unknown()))),
  completion_chart: z.optional(z.unknown()),
});
export type ArchivedEstimateDistribution = z.infer<typeof ArchivedEstimateDistribution>;

/**
 * Archived cycle detail for the peek body (ARCH-014): the list columns
 * plus the sub-issue count, the estimate sums and the analytics
 * summaries. The backend's extra model columns (view_props, sort_order,
 * external ids, snapshots, logo) are ignored — the peek never reads
 * them.
 */
export const ArchivedCycleDetail = z.object({
  ...ArchivedCycleListItem.shape,
  sub_issues: z.nullable(z.number()),
  completed_estimate_points: z.number(),
  total_estimate_points: z.number(),
  estimate_distribution: ArchivedEstimateDistribution,
  distribution: ArchivedDistribution,
});
export type ArchivedCycleDetail = z.infer<typeof ArchivedCycleDetail>;

/** GET .../archived-cycles/<id>/: one archived cycle for the peek. */
export async function getArchivedCycle(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string
): Promise<ArchivedCycleDetail> {
  const body = await client.getJson<unknown>(
    `/api/workspaces/${slug}/projects/${projectId}/archived-cycles/${cycleId}/`
  );
  return client.parse(ArchivedCycleDetail, body, `GET archived cycle ${cycleId}`);
}

/**
 * Cycle archive stamp: POST .../cycles/<id>/archive/ answers 200 with
 * the stamp. Only finished cycles archive (end_date in the past,
 * otherwise 400), so callers gate the entry on the COMPLETED status.
 * The backend str()s a datetime (space-separated, not ISO), so the
 * stamp stays a plain string like the project one.
 */
export const CycleArchiveStamp = z.object({
  archived_at: z.string(),
});
export type CycleArchiveStamp = z.infer<typeof CycleArchiveStamp>;

export async function archiveCycle(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string
): Promise<CycleArchiveStamp> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/archive/`;
  const envelope = await client.sendJson<unknown>("POST", path, {});
  return client.parse(CycleArchiveStamp, envelope.data, `POST ${path}`);
}

/**
 * DELETE .../cycles/<id>/archive/ clears the stamp and restores the
 * cycle. Resolves the status (204); the archived-cycles unarchive alias
 * 404s and is deliberately not contracted.
 */
export async function restoreCycle(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string
): Promise<number> {
  const response = await client.delete(`/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/archive/`);
  return response.status;
}
