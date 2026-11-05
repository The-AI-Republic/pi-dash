// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Run payloads as the backend sends them today (PIDASH-3). Migration
// runner/0032 dropped AgentRun.queue_position, so neither the issue
// detail's embedded run nor a runs-list item carries it. Each wire key
// list mirrors the serializer it names; the guard fails when a contract
// requires a key the backend no longer sends, which is how the issue
// page and the Runs list broke.
import { describe, expect, it } from "vitest";
import type { z } from "zod/mini";

import { AgentRunSummary } from "./issue-agent.js";
import { AgentRun } from "./runs.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";

/**
 * `_serialize_agent_run` keys (apps/api issue.py; rust-api
 * serializers_detail.rs AGENT_RUN_FIELDS), as the live issue detail's
 * agent_status.latest_run returns them.
 */
const ISSUE_RUN_WIRE_KEYS = [
  "id",
  "status",
  "executor_kind",
  "runner",
  "runner_name",
  "created_at",
  "assigned_at",
  "started_at",
  "ended_at",
  "done_payload",
  "error",
  "error_code",
  "error_diagnostic",
  "llm_model",
  "input_tokens",
  "output_tokens",
  "total_tokens",
  "live_state",
] as const;

/**
 * AgentRunSerializer.Meta.fields (apps/api runner/serializers.py; rust-api
 * runner_runs/shape.rs), as each GET /api/runners/runs/ item returns them.
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

const issueRun = {
  id: UUID,
  status: "completed",
  executor_kind: "local_runner",
  runner: UUID2,
  runner_name: "probe-runner",
  created_at: "2026-11-03T10:00:00Z",
  assigned_at: "2026-11-03T10:00:01Z",
  started_at: "2026-11-03T10:00:02Z",
  ended_at: "2026-11-03T10:05:00Z",
  done_payload: { outcome: "done" },
  error: "",
  error_code: "",
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

describe("run payloads without queue_position (PIDASH-3)", () => {
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

  it("requires no key the backend does not send", () => {
    expect(
      requiredKeys(AgentRunSummary).filter((key) => !(ISSUE_RUN_WIRE_KEYS as readonly string[]).includes(key))
    ).toEqual([]);
    expect(requiredKeys(AgentRun).filter((key) => !(RUN_LIST_WIRE_KEYS as readonly string[]).includes(key))).toEqual(
      []
    );
  });

  it("no longer declares queue_position", () => {
    expect(Object.keys(AgentRunSummary.shape)).not.toContain("queue_position");
    expect(Object.keys(AgentRun.shape)).not.toContain("queue_position");
  });
});
