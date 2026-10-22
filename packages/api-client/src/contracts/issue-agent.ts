// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Agent thin slice for the issues widgets: the typed agent-status
// read the issue detail carries, with its token figures. Run-level
// contracts (runs, dispatches, cancel, re-tick) live in the runners
// area; the widgets consume them from there.
import * as z from "zod/mini";
import { nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { DispatchWait, RunErrorDiagnostic } from "./runs.js";

/** The per-issue continuation clock as the detail read carries it. */
export const AgentTicker = z.object({
  enabled: z.boolean(),
  user_disabled: z.boolean(),
  used: z.number(),
  tick_count: z.number(),
  granted: z.number(),
  waited: z.number(),
  max_ticks: z.number(),
  remaining: z.nullable(z.number()),
  interval_seconds: z.number(),
  next_run_at: nullableDateTime(),
  last_tick_at: nullableDateTime(),
  disarm_reason: z.string(),
  pending_entry: z.boolean(),
  can_re_tick: z.boolean(),
});
export type AgentTicker = z.infer<typeof AgentTicker>;

/** Live runner heartbeat for the observed run, when one reports. */
export const AgentLiveState = z.object({
  observed_run_id: nullableUuid(),
  last_event_at: nullableDateTime(),
  last_event_kind: z.nullable(z.string()),
  last_event_summary: z.nullable(z.string()),
  agent_pid: z.nullable(z.number()),
  agent_subprocess_alive: z.nullable(z.boolean()),
  approvals_pending: z.nullable(z.number()),
  input_tokens: z.nullable(z.number()),
  output_tokens: z.nullable(z.number()),
  total_tokens: z.nullable(z.number()),
  llm_model: z.nullable(z.string()),
  turn_count: z.nullable(z.number()),
  updated_at: nullableDateTime(),
});
export type AgentLiveState = z.infer<typeof AgentLiveState>;

/** One run as the issue detail embeds it. */
export const AgentRunSummary = z.object({
  id: uuid(),
  status: z.string(),
  executor_kind: z.string(),
  queue_position: z.nullable(z.number()),
  runner: nullableUuid(),
  runner_name: z.nullable(z.string()),
  /**
   * Hybrid-assignment pin (design §14.3), mirroring the run contract.
   * Optional so backends that predate the waiting state still parse.
   */
  pinned_runner: z.optional(nullableUuid()),
  pinned_runner_name: z.optional(z.nullable(z.string())),
  pin_kind: z.optional(z.string()),
  dispatch_wait: z.optional(z.nullable(DispatchWait)),
  created_at: nullableDateTime(),
  assigned_at: nullableDateTime(),
  started_at: nullableDateTime(),
  ended_at: nullableDateTime(),
  done_payload: z.unknown(),
  error: z.string(),
  error_code: z.string(),
  error_diagnostic: z.nullable(RunErrorDiagnostic),
  llm_model: z.string(),
  input_tokens: z.nullable(z.number()),
  output_tokens: z.nullable(z.number()),
  total_tokens: z.nullable(z.number()),
  live_state: z.nullable(AgentLiveState),
});
export type AgentRunSummary = z.infer<typeof AgentRunSummary>;

/**
 * Agent status on the issue detail: null without runs or a ticker.
 * The cumulative token total rides optional until the backend
 * aggregate (main#446) lands; per-run and live token figures are
 * present today.
 */
export const AgentStatus = z.object({
  ticker: z.nullable(AgentTicker),
  active_run: z.nullable(AgentRunSummary),
  latest_run: z.nullable(AgentRunSummary),
  run_count: z.number(),
  total_tokens: z.optional(z.nullable(z.number())),
});
export type AgentStatus = z.infer<typeof AgentStatus>;
