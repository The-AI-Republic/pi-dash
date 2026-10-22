// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles: the full read/write surface for the cycles area (NEWFRONT-706).
// Shapes follow the cycles area spec §23 and the Django serializers it
// cites. The thin picker contract at the top predates the area and stays
// deliberately narrow: the palette and the issue pickers read only id,
// project, name, favorite and an optional lifecycle marker.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";

/**
 * Thin cycle row for the shell and the issues cycle picker: the palette's
 * Add-to-cycle picker lists names while the dropdown additionally filters
 * on status. The server already excludes archived cycles. Full cycle
 * ownership lives in the row schemas below.
 */
export const CycleListItem = z.object({
  id: uuid(),
  project_id: uuid(),
  name: z.string(),
  is_favorite: z.boolean(),
  // Lifecycle marker the cycle picker filters on (completed cycles are
  // excluded). The server sends it uppercase (DRAFT, CURRENT, UPCOMING,
  // COMPLETED), so readers compare case-insensitively. Optional: rows
  // already held by existing readers omit it, and those readers must keep
  // parsing.
  status: z.optional(z.string()),
});
export type CycleListItem = z.infer<typeof CycleListItem>;

export const CycleList = z.array(CycleListItem);
export type CycleList = z.infer<typeof CycleList>;

/** Lifecycle values the backend annotates; readers compare case-insensitively. */
export const CYCLE_STATUSES = ["DRAFT", "CURRENT", "UPCOMING", "COMPLETED"] as const;
export type CycleStatus = (typeof CYCLE_STATUSES)[number];

/**
 * One live cycle row (project list and detail). The server projects a
 * different column set per route, so only the fields every projection
 * carries are required:
 *
 * - `GET BASE/cycles/` (all and `cycle_view=current`) sends the counters,
 *   favorite flag, lifecycle marker and lead, but no `archived_at`, no
 *   `created_at`/`updated_at`, and `created_by` is a bare user id.
 * - The archived list adds the per-group counters and `archived_at`.
 * - The detail and archived-detail reads add the sub-issue count and the
 *   frozen estimate totals.
 * - The workspace list is a narrower projection of its own; see
 *   `WorkspaceCycleRow` below.
 *
 * Date instants arrive in the project timezone, hence the offset-aware
 * datetime helper rather than a Z-only pattern.
 */
export const CycleRow = z.object({
  id: uuid(),
  workspace_id: uuid(),
  project_id: uuid(),
  name: z.string(),
  description: z.optional(z.string()),
  start_date: nullableDateTime(),
  end_date: nullableDateTime(),
  owned_by_id: z.optional(nullableUuid()),
  view_props: z.optional(z.record(z.string(), z.unknown())),
  sort_order: z.optional(z.number()),
  external_source: z.optional(z.nullable(z.string())),
  external_id: z.optional(z.nullable(z.string())),
  progress_snapshot: z.optional(z.nullable(z.record(z.string(), z.unknown()))),
  logo_props: z.optional(z.record(z.string(), z.unknown())),
  is_favorite: z.boolean(),
  total_issues: z.optional(z.number()),
  completed_issues: z.optional(z.number()),
  cancelled_issues: z.optional(z.number()),
  started_issues: z.optional(z.number()),
  unstarted_issues: z.optional(z.number()),
  backlog_issues: z.optional(z.number()),
  assignee_ids: z.optional(z.array(uuid())),
  status: z.optional(z.string()),
  version: z.optional(z.number()),
  created_by: z.optional(nullableUuid()),
  created_at: z.optional(nullableDateTime()),
  updated_at: z.optional(nullableDateTime()),
  archived_at: z.optional(nullableDateTime()),
  /** Annotation on the detail and archived-detail reads. */
  sub_issues: z.optional(z.nullable(z.number())),
  /** Frozen estimate totals on the archived-detail read. */
  completed_estimate_points: z.optional(z.nullable(z.number())),
  total_estimate_points: z.optional(z.nullable(z.number())),
});
export type CycleRow = z.infer<typeof CycleRow>;

export const CycleRows = z.array(CycleRow);
export type CycleRows = z.infer<typeof CycleRows>;

/**
 * `GET WS/cycles/` answers its own projection, not the project-list row.
 * The backend builds counts on the queryset but never annotates the
 * favorite flag or lifecycle marker, so `is_favorite`, `status`,
 * `assignee_ids`, `version` and `created_by` are absent from the
 * response. Keep this separate from `CycleRow`; the workspace switcher
 * must not claim a favorite or lifecycle value the route never sends.
 */
export const WorkspaceCycleRow = z.strictObject({
  id: uuid(),
  workspace_id: uuid(),
  project_id: uuid(),
  name: z.string(),
  description: z.string(),
  start_date: nullableDateTime(),
  end_date: nullableDateTime(),
  owned_by_id: nullableUuid(),
  view_props: z.record(z.string(), z.unknown()),
  sort_order: z.number(),
  external_source: z.nullable(z.string()),
  external_id: z.nullable(z.string()),
  progress_snapshot: z.nullable(z.record(z.string(), z.unknown())),
  logo_props: z.record(z.string(), z.unknown()),
  total_issues: z.number(),
  cancelled_issues: z.number(),
  completed_issues: z.number(),
  started_issues: z.number(),
  unstarted_issues: z.number(),
  backlog_issues: z.number(),
});
export type WorkspaceCycleRow = z.infer<typeof WorkspaceCycleRow>;

export const WorkspaceCycleRows = z.array(WorkspaceCycleRow);
export type WorkspaceCycleRows = z.infer<typeof WorkspaceCycleRows>;

export interface CycleListQuery {
  /**
   * `all` (default) or `current`. The project cycles list never sends this;
   * only the hero's current-cycle read does. The picker and palette keep
   * using the plain all-cycle read.
   */
  cycleView?: "all" | "current";
  orderBy?: string;
  perPage?: number;
  cursor?: string;
  /** Comma-separated projection list the backend understands. */
  fields?: string;
}

export function cycleListQueryParams(
  query: CycleListQuery = {}
): Record<string, string | number | boolean | undefined | null> {
  return {
    cycle_view: query.cycleView,
    order_by: query.orderBy,
    per_page: query.perPage,
    cursor: query.cursor,
    fields: query.fields,
  };
}

/** Thin picker read, unchanged for existing readers; optional query passthrough added. */
export async function listCycles(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: CycleListQuery = {}
): Promise<CycleList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/cycles/`, {
    query: cycleListQueryParams(query),
  });
  return client.parse(CycleList, body, `GET cycles ${projectId}`);
}

/**
 * Full project cycle rows for the cycles area. The response is a bare array
 * (spec §23) — there is no page envelope on this route.
 */
export async function listCycleRows(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: CycleListQuery = {}
): Promise<CycleRows> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/cycles/`, {
    query: cycleListQueryParams(query),
  });
  return client.parse(CycleRows, body, `GET cycle rows ${projectId}`);
}

/** Create body: name is required; dates are both-or-neither (§6). */
export interface CycleCreateInput {
  name: string;
  description?: string;
  start_date?: string | null;
  end_date?: string | null;
  owned_by?: string | null;
  timezone?: string;
  external_source?: string | null;
  external_id?: string | null;
}

export async function createCycle(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: CycleCreateInput
): Promise<CycleRow> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/cycles/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(CycleRow, envelope.data, `POST ${path}`);
}

/** Detail read (peek and detail host). Missing ids are a real 404. */
export async function getCycle(client: ApiClient, slug: string, projectId: string, cycleId: string): Promise<CycleRow> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(CycleRow, body, `GET ${path}`);
}

/**
 * Thin update echo used by the palette: PATCH answers the updated
 * representation, whose stable core is these six fields. Callers needing
 * the full row re-read through `getCycle`.
 */
export const CycleUpdate = z.object({
  id: uuid(),
  name: z.string(),
  project_id: uuid(),
  description: z.optional(z.string()),
  start_date: nullableDateTime(),
  end_date: nullableDateTime(),
});
export type CycleUpdate = z.infer<typeof CycleUpdate>;

export interface CyclePatch {
  name?: string;
  description?: string;
  start_date?: string | null;
  end_date?: string | null;
  owned_by?: string | null;
  timezone?: string;
  sort_order?: number;
  external_source?: string | null;
  external_id?: string | null;
}

export async function updateCycle(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string,
  input: CyclePatch
): Promise<CycleUpdate> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(CycleUpdate, envelope.data, `PATCH cycle ${cycleId}`);
}

/** DELETE answers 204 with no body. */
export async function deleteCycle(client: ApiClient, slug: string, projectId: string, cycleId: string): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/`);
}

/**
 * Overlap check. `status: true` means the range is free; a collision comes
 * back 200 with `status: false` and a guidance message, not an HTTP error,
 * so callers read the flag and render the message.
 */
export const CycleDateCheck = z.object({
  status: z.boolean(),
  error: z.optional(z.string()),
});
export type CycleDateCheck = z.infer<typeof CycleDateCheck>;

export interface CycleDateCheckInput {
  /** yyyy-MM-dd in the project timezone, not a UTC instant (§23). */
  start_date: string;
  /** yyyy-MM-dd in the project timezone. */
  end_date: string;
  /** Set on edit so the cycle is excluded from its own overlap check. */
  cycle_id?: string;
}

export async function checkCycleDates(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: CycleDateCheckInput
): Promise<CycleDateCheck> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/cycles/date-check/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(CycleDateCheck, envelope.data, `POST ${path}`);
}

/** POST .../cycles/<id>/transfer-issues/ answers 200 {message}. */
export const CycleTransferResult = z.object({
  message: z.string(),
});
export type CycleTransferResult = z.infer<typeof CycleTransferResult>;

/**
 * Move a cycle's remaining issues into another cycle (ISS-224). The
 * transfer freezes the source progress snapshot, so the source keeps its
 * pre-transfer counts by design; the cycles area refreshes both rows.
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
 * Project-scoped favourite writes (CYC-029). These are NOT the toggle the
 * row star and palette use — those write the workspace favourites tree
 * (`sidebar.ts`: POST/DELETE WS/user-favorites/). Both answer 204 with no
 * body; the cycles list re-read carries `is_favorite`.
 */
export async function addCycleFavorite(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string
): Promise<void> {
  await client.post(`/api/workspaces/${slug}/projects/${projectId}/user-favorite-cycles/`, {
    json: { cycle: cycleId },
  });
}

export async function removeCycleFavorite(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string
): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/projects/${projectId}/user-favorite-cycles/${cycleId}/`);
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

export const ArchivedEstimateDistribution = z.union([
  z.strictObject({}),
  z.strictObject({
    assignees: z.array(z.record(z.string(), z.unknown())),
    labels: z.array(z.record(z.string(), z.unknown())),
    completion_chart: z.unknown(),
  }),
]);
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

/**
 * GET WS/cycles/ — workspace-wide rows for switchers and pickers. Bare
 * array with its own narrower projection (see `WorkspaceCycleRow`).
 */
export async function listWorkspaceCycles(client: ApiClient, slug: string): Promise<WorkspaceCycleRows> {
  const path = `/api/workspaces/${slug}/cycles/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(WorkspaceCycleRows, body, `GET ${path}`);
}

/**
 * Cursor-paged envelope used by the workspace active-cycles read. The page
 * itself is an upsell in this checkout and never calls the route, and the
 * OSS backend serves no `active-cycles/` path, so the live proof needs the
 * cloud backend; unit stubs pin the URL and envelope.
 */
export const CycleCursorPage = z.object({
  count: z.number(),
  next_cursor: z.nullable(z.string()),
  prev_cursor: z.nullable(z.string()),
  results: CycleRows,
  next_page_results: z.optional(z.boolean()),
  prev_page_results: z.optional(z.boolean()),
  total_results: z.optional(z.number()),
  total_count: z.optional(z.number()),
  total_pages: z.optional(z.number()),
});
export type CycleCursorPage = z.infer<typeof CycleCursorPage>;

export interface CycleCursorQuery {
  perPage?: number;
  cursor?: string;
}

export async function listActiveCycles(
  client: ApiClient,
  slug: string,
  query: CycleCursorQuery = {}
): Promise<CycleCursorPage> {
  const path = `/api/workspaces/${slug}/active-cycles/`;
  const body = await client.getJson<unknown>(path, {
    query: { per_page: query.perPage, cursor: query.cursor },
  });
  return client.parse(CycleCursorPage, body, `GET ${path}`);
}

/**
 * The progress counts the completed-cycle transfer prompt, ring and sidebar
 * read (CYC-038/041/045). Once a transfer runs the server freezes the
 * pre-move counts into the cycle's `progress_snapshot` and serves those.
 */
export const CycleProgress = z.object({
  total_issues: z.number(),
  backlog_issues: z.number(),
  unstarted_issues: z.number(),
  started_issues: z.number(),
  completed_issues: z.number(),
  cancelled_issues: z.number(),
  backlog_estimate_points: z.number(),
  unstarted_estimate_points: z.number(),
  started_estimate_points: z.number(),
  completed_estimate_points: z.number(),
  cancelled_estimate_points: z.number(),
  total_estimate_points: z.nullable(z.number()),
});
export type CycleProgress = z.infer<typeof CycleProgress>;

/** GET .../cycles/<id>/progress/: snapshot and per-group counts. */
export async function getCycleProgress(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string
): Promise<CycleProgress> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/progress/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(CycleProgress, body, `GET cycle progress ${cycleId}`);
}

/** One series point: per-date remaining work, null beyond today. */
export const CycleProgressPoint = z.object({
  date: z.string(),
  value: z.nullable(z.number()),
});
export type CycleProgressPoint = z.infer<typeof CycleProgressPoint>;

/**
 * Cloud-only detailed progress read (`/cycle-progress/`). The OSS backend
 * serves no such route; the shape is the snapshot counts plus the chart
 * series and distribution the cloud view renders. Live proof needs the
 * cloud backend — the unit stub pins the URL and shape.
 */
export const CycleDetailedProgress = z.object({
  ...CycleProgress.shape,
  distribution: z.optional(z.unknown()),
  progress: z.optional(z.array(CycleProgressPoint)),
});
export type CycleDetailedProgress = z.infer<typeof CycleDetailedProgress>;

export async function getCycleDetailedProgress(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string
): Promise<CycleDetailedProgress> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/cycle-progress/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(CycleDetailedProgress, body, `GET ${path}`);
}

/** One assignee row in the analytics breakdown (issues or points measure). */
export const CycleAssigneeDistribution = z.object({
  display_name: z.optional(z.nullable(z.string())),
  assignee_id: z.optional(nullableUuid()),
  avatar_url: z.optional(z.nullable(z.string())),
  total_issues: z.optional(z.nullable(z.number())),
  completed_issues: z.optional(z.nullable(z.number())),
  pending_issues: z.optional(z.nullable(z.number())),
  total_estimates: z.optional(z.nullable(z.number())),
  completed_estimates: z.optional(z.nullable(z.number())),
  pending_estimates: z.optional(z.nullable(z.number())),
});
export type CycleAssigneeDistribution = z.infer<typeof CycleAssigneeDistribution>;

/** One label row in the analytics breakdown. */
export const CycleLabelDistribution = z.object({
  label_name: z.optional(z.nullable(z.string())),
  label_id: z.optional(nullableUuid()),
  color: z.optional(z.nullable(z.string())),
  total_issues: z.optional(z.nullable(z.number())),
  completed_issues: z.optional(z.nullable(z.number())),
  pending_issues: z.optional(z.nullable(z.number())),
  total_estimates: z.optional(z.nullable(z.number())),
  completed_estimates: z.optional(z.nullable(z.number())),
  pending_estimates: z.optional(z.nullable(z.number())),
});
export type CycleLabelDistribution = z.infer<typeof CycleLabelDistribution>;

/**
 * Burndown series: date key (`yyyy-MM-dd`) to remaining work. Future dates
 * carry null, so the chart stops at today.
 */
export const CycleCompletionChart = z.record(z.string(), z.nullable(z.number()));
export type CycleCompletionChart = z.infer<typeof CycleCompletionChart>;

/** GET .../analytics?type=issues|points — breakdown distributions (CYC-038/041). */
export const CycleDistribution = z.object({
  assignees: z.array(CycleAssigneeDistribution),
  labels: z.array(CycleLabelDistribution),
  completion_chart: CycleCompletionChart,
});
export type CycleDistribution = z.infer<typeof CycleDistribution>;

export const CYCLE_ANALYTICS_TYPES = ["issues", "points"] as const;
export type CycleAnalyticsType = (typeof CYCLE_ANALYTICS_TYPES)[number];

export async function getCycleAnalytics(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string,
  type: CycleAnalyticsType = "issues"
): Promise<CycleDistribution> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/analytics/`;
  const body = await client.getJson<unknown>(path, { query: { type } });
  return client.parse(CycleDistribution, body, `GET cycle analytics ${cycleId}`);
}
