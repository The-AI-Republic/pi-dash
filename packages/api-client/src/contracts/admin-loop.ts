// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Instance-admin loop contracts: job list/create/detail/update/delete plus
// the per-job targets read. The admin surface exposes the full job
// (prompt, minimum role, recurrence); unlike member reads it never hides
// the prompt.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { uuid } from "./fields.js";

/** Minimum-role values the backend accepts (admin/member/guest). */
export const LOOP_MIN_ROLES = [20, 15, 5] as const;
export type LoopMinRole = (typeof LOOP_MIN_ROLES)[number];

/** Fixed skip-reason set for the targets filter (blank means all). */
export const LOOP_SKIP_REASONS = [
  "user_disabled",
  "master_paused",
  "min_role",
  "llm_config_missing",
  "membership_gone",
  "turn_active",
  "dispatch_error",
] as const;
export type LoopSkipReason = (typeof LOOP_SKIP_REASONS)[number];

/** One loop job as the list/create/update endpoints report it (no stats). */
export const LoopJob = z.object({
  id: uuid(),
  slug: z.string(),
  name: z.string(),
  public_name: z.string(),
  public_description: z.string(),
  prompt: z.string(),
  min_role: z.number(),
  enabled: z.boolean(),
  is_builtin: z.boolean(),
  dtstart: z.nullable(z.string()),
  rrule: z.string(),
  tzid: z.string(),
  created_at: z.nullable(z.string()),
  updated_at: z.nullable(z.string()),
});
export type LoopJob = z.infer<typeof LoopJob>;

export const LoopJobList = z.array(LoopJob);
export type LoopJobList = z.infer<typeof LoopJobList>;

/** 24h rollup the detail read annotates onto the job. */
export const LoopJobStats = z.object({
  target_count: z.number(),
  completed: z.number(),
  failed: z.number(),
  skipped: z.number(),
});
export type LoopJobStats = z.infer<typeof LoopJobStats>;

/** Detail read: the job plus its 24h stats rollup. */
export const LoopJobDetail = z.object({
  ...LoopJob.shape,
  stats: LoopJobStats,
});
export type LoopJobDetail = z.infer<typeof LoopJobDetail>;

export async function listLoopJobs(client: ApiClient): Promise<LoopJobList> {
  const path = "/api/instances/loop/jobs/";
  const body = await client.getJson<unknown>(path);
  return client.parse(LoopJobList, body, `GET ${path}`);
}

export interface LoopJobCreateInput {
  slug: string;
  name: string;
  public_name: string;
  public_description?: string;
  prompt: string;
  min_role?: number;
  enabled?: boolean;
  rrule: string;
  tzid?: string;
}

export async function createLoopJob(client: ApiClient, input: LoopJobCreateInput): Promise<LoopJob> {
  const path = "/api/instances/loop/jobs/";
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(LoopJob, envelope.data, `POST ${path}`);
}

export async function getLoopJob(client: ApiClient, jobId: string): Promise<LoopJobDetail> {
  const path = `/api/instances/loop/jobs/${jobId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(LoopJobDetail, body, `GET ${path}`);
}

export interface LoopJobPatchInput {
  slug?: string;
  name?: string;
  public_name?: string;
  public_description?: string;
  prompt?: string;
  min_role?: number;
  enabled?: boolean;
  rrule?: string;
  tzid?: string;
}

export async function updateLoopJob(client: ApiClient, jobId: string, input: LoopJobPatchInput): Promise<LoopJob> {
  const path = `/api/instances/loop/jobs/${jobId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(LoopJob, envelope.data, `PATCH ${path}`);
}

export async function deleteLoopJob(client: ApiClient, jobId: string): Promise<number> {
  const response = await client.delete(`/api/instances/loop/jobs/${jobId}/`);
  return response.status;
}

/** Last-run projection on a target row (null when the target never ran). */
export const LoopTargetLastRun = z.object({
  status: z.string(),
  error_code: z.string(),
  model_used: z.string(),
  total_tokens: z.nullable(z.number()),
  completed_at: z.nullable(z.string()),
});
export type LoopTargetLastRun = z.infer<typeof LoopTargetLastRun>;

/** One (job x workspace x user) target edge as the targets read reports it. */
export const LoopTarget = z.object({
  id: uuid(),
  workspace_slug: z.nullable(z.string()),
  user_email: z.nullable(z.string()),
  next_run_at: z.nullable(z.string()),
  last_skipped_at: z.nullable(z.string()),
  last_skip_reason: z.string(),
  last_run: z.nullable(LoopTargetLastRun),
});
export type LoopTarget = z.infer<typeof LoopTarget>;

/** Targets page (fifty per page). */
export const LoopTargetsPage = z.object({
  page: z.number(),
  results: z.array(LoopTarget),
});
export type LoopTargetsPage = z.infer<typeof LoopTargetsPage>;

export interface LoopTargetsQuery {
  page?: number;
  skipReason?: string;
}

export async function listLoopTargets(
  client: ApiClient,
  jobId: string,
  query: LoopTargetsQuery = {}
): Promise<LoopTargetsPage> {
  const path = `/api/instances/loop/jobs/${jobId}/targets/`;
  const body = await client.getJson<unknown>(path, {
    query: {
      page: query.page,
      skip_reason: query.skipReason === undefined || query.skipReason === "" ? undefined : query.skipReason,
    },
  });
  return client.parse(LoopTargetsPage, body, `GET ${path}`);
}
