// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Run contracts: the paged runs list, the detail with its event
// transcript and diagnostics, cancellation, dispatch (free-form plus
// the Run AI and Comment & Run issue flows), and the issue re-tick
// that re-grants a spent run budget.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { DispatchWait, RunErrorDiagnostic } from "./issue-agent.js";
import { PodMini } from "./pods.js";

export const RUN_STATUSES = [
  "queued",
  "assigned",
  "waiting_for_worktree",
  "running",
  "cancel_requested",
  "awaiting_approval",
  "awaiting_reauth",
  "paused_awaiting_input",
  "blocked",
  "completed",
  "failed",
  "cancelled",
  "refused",
] as const;
export type RunStatus = (typeof RUN_STATUSES)[number];

export const EXECUTOR_KINDS = ["local_runner", "cloud_agent", "managed_runner"] as const;
export type ExecutorKind = (typeof EXECUTOR_KINDS)[number];

export const TOOL_CALL_STATUSES = ["prepared", "submitted", "succeeded", "failed", "unknown", "denied"] as const;
export type ToolCallStatus = (typeof TOOL_CALL_STATUSES)[number];

/** One executed tool call on a cloud-agent run. */
export const RunToolCall = z.object({
  id: uuid(),
  tool_call_id: z.string(),
  source: z.string(),
  server_key: z.string(),
  tool_name: z.string(),
  risk: z.string(),
  status: z.enum(TOOL_CALL_STATUSES),
  error_code: z.string(),
  prepared_at: dateTime(),
  submitted_at: nullableDateTime(),
  completed_at: nullableDateTime(),
});
export type RunToolCall = z.infer<typeof RunToolCall>;

/** One sequenced transcript event streamed from the runner. */
export const RunEvent = z.object({
  id: z.number(),
  seq: z.number(),
  kind: z.string(),
  payload: z.record(z.string(), z.unknown()),
  created_at: dateTime(),
});
export type RunEvent = z.infer<typeof RunEvent>;

/** Which scheduler binding fired a run, for the back-link. */
export const RunSchedulerBinding = z.object({
  id: uuid(),
  scheduler_name: z.string(),
  scheduler_slug: z.string(),
  project: uuid(),
});
export type RunSchedulerBinding = z.infer<typeof RunSchedulerBinding>;

/** Planned tools and limits for a cloud-agent run. */
export const RunToolPlan = z.object({
  tools: z.optional(z.array(z.string())),
  unavailable_capabilities: z.optional(z.array(z.string())),
  limits: z.optional(z.record(z.string(), z.number())),
});
export type RunToolPlan = z.infer<typeof RunToolPlan>;

/**
 * One agent run. `events` rides along only when the detail fetch asks
 * for it; `tool_calls` is empty off the cloud-agent executor.
 */
export const AgentRun = z.object({
  id: uuid(),
  status: z.enum(RUN_STATUSES),
  executor_kind: z.enum(EXECUTOR_KINDS),
  dispatch_attempts: z.number(),
  cancel_requested_at: nullableDateTime(),
  cancel_reason: z.string(),
  prompt: z.string(),
  thread_id: z.string(),
  agent_metadata: z.record(z.string(), z.unknown()),
  runner: nullableUuid(),
  work_item: nullableUuid(),
  /**
   * Hybrid-assignment pin (design §14.3): the runner this queued run
   * waits for, its name, and the pin kind (locality or dedicated);
   * dispatch_wait explains the wait. Optional so backends that predate
   * the waiting state still parse.
   */
  pinned_runner: z.optional(nullableUuid()),
  pinned_runner_name: z.optional(z.nullable(z.string())),
  pin_kind: z.optional(z.string()),
  dispatch_wait: z.optional(z.nullable(DispatchWait)),
  scheduler_binding: nullableUuid(),
  scheduler_binding_detail: z.nullable(RunSchedulerBinding),
  pod: uuid(),
  pod_detail: PodMini,
  created_by: uuid(),
  owner: nullableUuid(),
  created_at: dateTime(),
  assigned_at: nullableDateTime(),
  queue_position: z.nullable(z.number()),
  started_at: nullableDateTime(),
  ended_at: nullableDateTime(),
  done_payload: z.nullable(z.record(z.string(), z.unknown())),
  error: z.string(),
  error_code: z.string(),
  error_diagnostic: z.nullable(RunErrorDiagnostic),
  refusal_category: z.string(),
  llm_model: z.string(),
  input_tokens: z.nullable(z.number()),
  output_tokens: z.nullable(z.number()),
  total_tokens: z.nullable(z.number()),
  usage: z.record(z.string(), z.unknown()),
  tool_plan: RunToolPlan,
  tool_calls: z.array(RunToolCall),
  events: z.optional(z.array(RunEvent)),
});
export type AgentRun = z.infer<typeof AgentRun>;

/** One page of runs; `page` is 1-based, `count` the page length. */
export const AgentRunPage = z.object({
  results: z.array(AgentRun),
  count: z.number(),
  total_count: z.number(),
  total_pages: z.number(),
  page: z.number(),
  per_page: z.number(),
});
export type AgentRunPage = z.infer<typeof AgentRunPage>;

export interface RunListQuery {
  workspaceId?: string;
  projectId?: string;
  schedulerBindingId?: string;
  page?: number;
  perPage?: number;
}

/**
 * Runs the caller is involved with, newest first. Out-of-range pages
 * answer an empty `results` with the real `total_pages`.
 */
export async function listRuns(client: ApiClient, query: RunListQuery = {}): Promise<AgentRunPage> {
  const path = "/api/runners/runs/";
  const body = await client.getJson<unknown>(path, {
    query: {
      workspace: query.workspaceId,
      project: query.projectId,
      scheduler_binding: query.schedulerBindingId,
      page: query.page,
      per_page: query.perPage,
    },
  });
  return client.parse(AgentRunPage, body, `GET ${path}`);
}

/** One run by id; `includeEvents` appends the transcript, oldest first. */
export async function getRun(
  client: ApiClient,
  runId: string,
  options: { includeEvents?: boolean } = {}
): Promise<AgentRun> {
  const path = `/api/runners/runs/${runId}/`;
  const body = await client.getJson<unknown>(path, {
    query: { include_events: options.includeEvents === true ? "1" : undefined },
  });
  return client.parse(AgentRun, body, `GET ${path}`);
}

export interface RunCreateInput {
  workspace: string;
  prompt: string;
  work_item?: string;
  /** Pod override; otherwise resolved from the issue or the default. */
  pod?: string;
  run_config?: Record<string, unknown>;
  required_capabilities?: string[];
}

/**
 * Start a free-form run. The prompt is composed server-side around
 * the given text; the executor follows project policy. Answers 201.
 */
export async function createRun(client: ApiClient, input: RunCreateInput): Promise<AgentRun> {
  const path = "/api/runners/runs/";
  const envelope = await client.sendJson<unknown>("POST", path, { json: { ...input } });
  return client.parse(AgentRun, envelope.data, `POST ${path}`);
}

export interface DispatchInput {
  workspace: string;
  work_item: string;
}

/**
 * 202 shape when a dispatch lands while a run is already active: the
 * next run is queued and starts when the issue frees up.
 */
export const AgentRunQueued = z.object({
  queued: z.literal(true),
  reason: z.string(),
  detail: z.string(),
});
export type AgentRunQueued = z.infer<typeof AgentRunQueued>;

/** Dispatch outcome: the fresh run, or the queued-run notice. */
export const AgentRunCreateResult = z.union([AgentRun, AgentRunQueued]);
export type AgentRunCreateResult = z.infer<typeof AgentRunCreateResult>;

/**
 * Run AI on an issue: the prompt is rendered server-side from the
 * issue's phase template. Answers 201, or the 202 queued shape.
 */
export async function runAi(client: ApiClient, input: DispatchInput): Promise<AgentRunCreateResult> {
  const path = "/api/runners/runs/";
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: { ...input, triggered_by: "run_ai" },
  });
  return client.parse(AgentRunCreateResult, envelope.data, `POST ${path}`);
}

/**
 * Comment & Run on an issue: same dispatch path as Run AI, through
 * the per-issue continuation pipeline. The comment must already be
 * posted; the prompt is rebuilt from the thread at dispatch time.
 */
export async function commentAndRun(client: ApiClient, input: DispatchInput): Promise<AgentRunCreateResult> {
  const path = "/api/runners/runs/";
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: { ...input, triggered_by: "comment_and_run" },
  });
  return client.parse(AgentRunCreateResult, envelope.data, `POST ${path}`);
}

/**
 * Cancel a run. Queued runs settle to cancelled at once; runs with
 * an agent process move to cancel-requested until the runner
 * confirms. Answers the updated run; an already-terminal run is
 * refused with 409.
 */
export async function cancelRun(client: ApiClient, runId: string, reason?: string): Promise<AgentRun> {
  const path = `/api/runners/runs/${runId}/cancel/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { reason: reason ?? "" } });
  return client.parse(AgentRun, envelope.data, `POST ${path}`);
}

/**
 * Release-pin outcome (hybrid assignment, design §12.2): the updated
 * run — still queued, now unpinned — plus, on newer backends, the
 * assignment note confirming the issue's dedicated runner is unchanged.
 */
export const ReleaseRunPinResult = z.object({
  ...AgentRun.shape,
  assignment: z.optional(
    z.object({
      assigned_runner_id: nullableUuid(),
      changed: z.boolean(),
    })
  ),
});
export type ReleaseRunPinResult = z.infer<typeof ReleaseRunPinResult>;

/**
 * Release one queued pinned run to its pod's shared queue. Run-scoped:
 * the issue's dedicated assignment is unchanged (design §12.2). Only
 * the issue's creator and current assignees may release a dedicated
 * pin (403 otherwise); non-queued or unpinned runs are refused with
 * 409. Answers the updated run.
 */
export async function releaseRunPin(client: ApiClient, runId: string): Promise<ReleaseRunPinResult> {
  const path = `/api/runners/runs/${runId}/release-pin/`;
  const envelope = await client.sendJson<unknown>("POST", path, {});
  return client.parse(ReleaseRunPinResult, envelope.data, `POST ${path}`);
}

/**
 * Re-tick outcome. A grant carries the refreshed budget counters and
 * the run it started, if any; a refusal is a normal 200 outcome with
 * a machine-readable reason, not an error.
 */
export const ReTickResponse = z.object({
  granted: z.boolean(),
  reason: z.string(),
  run_id: nullableUuid(),
  used: z.optional(z.number()),
  tick_count: z.optional(z.number()),
  max_ticks: z.optional(z.number()),
  enabled: z.optional(z.boolean()),
  pending_entry: z.optional(z.boolean()),
  next_run_at: z.optional(nullableDateTime()),
});
export type ReTickResponse = z.infer<typeof ReTickResponse>;

/**
 * Re-grant run budget to an issue whose pool is spent. The server
 * decides: outside a ticking state or a spent pool this answers a
 * refusal. A human action; agent runs cannot re-tick themselves.
 */
export async function reTickIssue(client: ApiClient, input: { work_item: string }): Promise<ReTickResponse> {
  const path = "/api/runners/re-tick/";
  const envelope = await client.sendJson<unknown>("POST", path, { json: { ...input } });
  return client.parse(ReTickResponse, envelope.data, `POST ${path}`);
}
