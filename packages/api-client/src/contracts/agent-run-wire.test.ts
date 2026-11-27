// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Run payloads as the backends send them today. Migration runner/0032
// dropped AgentRun.queue_position, so neither the issue detail's embedded
// run nor a runs-list item carries it; the contracts keep it optional for
// older servers. The guard fails when a contract requires a key either
// backend no longer sends, which is how the issue page and the Runs list
// broke.
//
// The wire key lists below are hand copies of the serializers they name.
// Nothing here re-reads those serializers, so a backend change must be
// copied in by hand; the live contract suites (src/contract/, skipped
// without PIDASH_CONTRACT_BASE_URL) are what compare against a real server.
import { describe, expect, it } from "vitest";
import type { z } from "zod/mini";

import { AgentRunSummary } from "./issue-agent.js";
import { AgentRun } from "./runs.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";

/**
 * `_serialize_agent_run` keys (apps/api app/serializers/issue.py), as the
 * Django issue detail's agent_status.latest_run returns them.
 */
const ISSUE_RUN_WIRE_KEYS = [
  "id",
  "status",
  "executor_kind",
  "runner",
  "runner_name",
  "pinned_runner",
  "pinned_runner_name",
  "pin_kind",
  "dispatch_wait",
  "created_at",
  "assigned_at",
  "started_at",
  "ended_at",
  "cancel_requested_at",
  "cancel_reason",
  "done_payload",
  "error",
  "error_code",
  "failure_reason",
  "error_diagnostic",
  "llm_model",
  "input_tokens",
  "output_tokens",
  "total_tokens",
  "live_state",
] as const;

/** rust-api serializers_detail.rs AGENT_RUN_FIELDS. */
const RUST_ISSUE_RUN_WIRE_KEYS = [
  "id",
  "status",
  "executor_kind",
  "runner",
  "runner_name",
  "created_at",
  "assigned_at",
  "started_at",
  "ended_at",
  "cancel_requested_at",
  "cancel_reason",
  "done_payload",
  "error",
  "error_code",
  "failure_reason",
  "error_diagnostic",
  "llm_model",
  "input_tokens",
  "output_tokens",
  "total_tokens",
  "live_state",
] as const;

/**
 * AgentRunSerializer.Meta.fields (apps/api runner/serializers.py), as each
 * Django GET /api/runners/runs/ item returns them.
 */
const RUN_LIST_WIRE_KEYS = [
  "id",
  "status",
  "executor_kind",
  "dispatch_attempts",
  "cancel_requested_at",
  "cancel_reason",
  "prompt",
  "thread_id",
  "agent_metadata",
  "runner",
  "pinned_runner",
  "pinned_runner_name",
  "pin_kind",
  "dispatch_wait",
  "work_item",
  "scheduler_binding",
  "scheduler_binding_detail",
  "pod",
  "pod_detail",
  "created_by",
  "owner",
  "created_at",
  "assigned_at",
  "started_at",
  "ended_at",
  "done_payload",
  "error",
  "error_code",
  "failure_reason",
  "error_diagnostic",
  "refusal_category",
  "llm_model",
  "input_tokens",
  "output_tokens",
  "total_tokens",
  "usage",
  "tool_plan",
  "tool_calls",
] as const;

/** rust-api runner_runs/shape.rs run fields (adds capacity_wait). */
const RUST_RUN_LIST_WIRE_KEYS = [
  ...RUN_LIST_WIRE_KEYS.slice(0, 14),
  "capacity_wait",
  ...RUN_LIST_WIRE_KEYS.slice(14),
] as const;

const issueRun = {
  id: UUID,
  status: "completed",
  executor_kind: "local_runner",
  runner: UUID2,
  runner_name: "probe-runner",
  pinned_runner: null,
  pinned_runner_name: null,
  pin_kind: "locality",
  dispatch_wait: null,
  created_at: "2026-11-03T10:00:00Z",
  assigned_at: "2026-11-03T10:00:01Z",
  started_at: "2026-11-03T10:00:02Z",
  ended_at: "2026-11-03T10:05:00Z",
  cancel_requested_at: null,
  cancel_reason: "",
  done_payload: { outcome: "done" },
  error: "",
  error_code: "",
  failure_reason: "",
  error_diagnostic: null,
  llm_model: "probe-model",
  input_tokens: 10,
  output_tokens: 20,
  total_tokens: 30,
  live_state: null,
};

const listRun = {
  id: UUID,
  status: "completed",
  executor_kind: "local_runner",
  dispatch_attempts: 1,
  cancel_requested_at: null,
  cancel_reason: "",
  prompt: "Probe prompt",
  thread_id: "thread-probe",
  agent_metadata: {},
  runner: UUID2,
  pinned_runner: null,
  pinned_runner_name: null,
  pin_kind: "locality",
  dispatch_wait: null,
  work_item: UUID2,
  scheduler_binding: null,
  scheduler_binding_detail: null,
  pod: UUID,
  pod_detail: { id: UUID, name: "CT_pod_1", is_default: true, project: UUID2, project_identifier: "CT" },
  created_by: UUID2,
  owner: UUID2,
  created_at: "2026-11-03T10:00:00Z",
  assigned_at: "2026-11-03T10:00:01Z",
  started_at: "2026-11-03T10:00:02Z",
  ended_at: "2026-11-03T10:05:00Z",
  done_payload: null,
  error: "",
  error_code: "",
  failure_reason: "",
  error_diagnostic: null,
  refusal_category: "",
  llm_model: "probe-model",
  input_tokens: 10,
  output_tokens: 20,
  total_tokens: 30,
  usage: {},
  tool_plan: {},
  tool_calls: [],
};

/** Keys the schema rejects when absent. */
function requiredKeys(schema: { shape: Record<string, z.ZodMiniType> }): string[] {
  return Object.entries(schema.shape)
    .filter(([, field]) => !field.safeParse(undefined).success)
    .map(([key]) => key);
}

describe("run payloads without queue_position", () => {
  it("fixtures carry exactly the wire keys", () => {
    expect(Object.keys(issueRun)).toEqual([...ISSUE_RUN_WIRE_KEYS]);
    expect(Object.keys(listRun)).toEqual([...RUN_LIST_WIRE_KEYS]);
  });

  it("parses the issue detail's embedded run", () => {
    const parsed = AgentRunSummary.safeParse(issueRun);
    expect(parsed.error?.issues).toBeUndefined();
    expect(parsed.success).toBe(true);
  });

  it("parses a runs-list item", () => {
    const parsed = AgentRun.safeParse(listRun);
    expect(parsed.error?.issues).toBeUndefined();
    expect(parsed.success).toBe(true);
  });

  it("requires no key either backend does not send", () => {
    const missing = (schema: Parameters<typeof requiredKeys>[0], wire: readonly string[]) =>
      requiredKeys(schema).filter((key) => !wire.includes(key));
    expect(missing(AgentRunSummary, ISSUE_RUN_WIRE_KEYS)).toEqual([]);
    expect(missing(AgentRunSummary, RUST_ISSUE_RUN_WIRE_KEYS)).toEqual([]);
    expect(missing(AgentRun, RUN_LIST_WIRE_KEYS)).toEqual([]);
    expect(missing(AgentRun, RUST_RUN_LIST_WIRE_KEYS)).toEqual([]);
  });

  it("parses the Rust spellings of both payloads", () => {
    const pick = (source: Record<string, unknown>, keys: readonly string[]) =>
      Object.fromEntries(keys.map((key) => [key, source[key] ?? null]));
    expect(AgentRunSummary.safeParse(pick(issueRun, RUST_ISSUE_RUN_WIRE_KEYS)).error?.issues).toBeUndefined();
    expect(AgentRun.safeParse(pick(listRun, RUST_RUN_LIST_WIRE_KEYS)).error?.issues).toBeUndefined();
  });

  it("does not require queue_position but still accepts it from older servers", () => {
    expect(requiredKeys(AgentRunSummary)).not.toContain("queue_position");
    expect(requiredKeys(AgentRun)).not.toContain("queue_position");
    expect(AgentRunSummary.safeParse({ ...issueRun, queue_position: 3 }).success).toBe(true);
    expect(AgentRun.safeParse({ ...listRun, queue_position: null }).success).toBe(true);
  });
});
