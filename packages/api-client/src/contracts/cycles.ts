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
import { nullableDateTime, uuid } from "./fields.js";

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
 * The fields the completed-cycle transfer prompt reads (ISS-224). Once a
 * transfer runs, the server freezes the pre-move counts into
 * `progress_snapshot`; a snapshot that is present (non-empty) means the
 * cycle can no longer transfer. The four issue counts are the pre-freeze
 * live progress the detail serializer also carries.
 */
export const CycleDetail = z.object({
  id: uuid(),
  project_id: uuid(),
  name: z.string(),
  description: z.optional(z.string()),
  start_date: nullableDateTime(),
  end_date: nullableDateTime(),
  // Uppercase lifecycle marker (DRAFT, ACTIVE, COMPLETED, ...).
  status: z.optional(z.string()),
  progress_snapshot: z.optional(z.nullable(z.record(z.string(), z.unknown()))),
  total_issues: z.optional(z.number()),
  backlog_issues: z.optional(z.number()),
  unstarted_issues: z.optional(z.number()),
  started_issues: z.optional(z.number()),
});
export type CycleDetail = z.infer<typeof CycleDetail>;

/** GET .../cycles/<id>/: the cycle with its progress snapshot and counts. */
export async function getCycle(
  client: ApiClient,
  slug: string,
  projectId: string,
  cycleId: string
): Promise<CycleDetail> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/projects/${projectId}/cycles/${cycleId}/`);
  return client.parse(CycleDetail, body, `GET cycle ${cycleId}`);
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
