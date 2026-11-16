/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { describe, expect, it, vi } from "vitest";
import type { IRunnerLiveState, TIssueAgentRunSummary } from "@pi-dash/types";

vi.mock("@pi-dash/constants", () => ({ API_BASE_URL: "" }));
vi.mock("@pi-dash/utils", () => ({ cn: (...args: unknown[]) => args.filter(Boolean).join(" ") }));
vi.mock("@pi-dash/propel/toast", () => ({
  TOAST_TYPE: { SUCCESS: "success", ERROR: "error", INFO: "info" },
  setToast: vi.fn(),
}));
vi.mock("@pi-dash/propel/badge", () => ({ Badge: () => null }));
vi.mock("@pi-dash/propel/button", () => ({ Button: () => null }));
vi.mock("@pi-dash/ui", () => ({ AlertModalCore: () => null }));
vi.mock("@/services/runner", () => ({
  AgentRunService: class {
    abortRun = vi.fn();
    reTick = vi.fn();
  },
}));

import { getRunView } from "../../core/components/issues/issue-detail/agent-status";

/** Substitute `{name}` placeholders, matching the app's translation shim. */
const t = ((key: string, vars?: Record<string, unknown>) =>
  vars ? key.replace(/\{(\w+)\}/g, (_m, k) => String(vars[k] ?? "")) : key) as never;

const NOW = new Date("2026-10-19T12:00:00Z").getTime();

function liveState(over: Partial<IRunnerLiveState>): IRunnerLiveState {
  return {
    observed_run_id: "run-1",
    last_event_at: null,
    last_event_kind: null,
    last_event_summary: null,
    agent_pid: null,
    agent_subprocess_alive: null,
    approvals_pending: null,
    input_tokens: null,
    output_tokens: null,
    total_tokens: null,
    llm_model: null,
    turn_count: null,
    agent_health: null,
    agent_last_activity_at: null,
    updated_at: new Date(NOW).toISOString(),
    ...over,
  };
}

function run(over: Partial<TIssueAgentRunSummary>): TIssueAgentRunSummary {
  return {
    id: "run-1",
    status: "running",
    executor_kind: "local_runner",
    runner: "r1",
    runner_name: "runner one",
    created_at: new Date(NOW - 3600_000).toISOString(),
    assigned_at: null,
    started_at: null,
    ended_at: new Date(NOW - 60_000).toISOString(),
    done_payload: null,
    error: "",
    error_code: "",
    error_diagnostic: null,
    llm_model: "",
    input_tokens: null,
    output_tokens: null,
    total_tokens: null,
    ...over,
  };
}

describe("getRunView agent health", () => {
  it("flags a running run the runner reports unhealthy", () => {
    const view = getRunView(
      run({
        live_state: liveState({
          agent_health: "unhealthy",
          agent_last_activity_at: new Date(NOW - 25 * 3600_000).toISOString(),
        }),
      }),
      null,
      1,
      NOW,
      t
    );
    expect(view.title).toBe("AI agent has gone quiet");
    expect(view.badge).toBe("Unhealthy");
    expect(view.badgeVariant).toBe("warning");
    expect(view.detail).toContain("Last activity");
  });

  it("keeps the working view for a healthy run and reads the watchdog's activity stamp", () => {
    const view = getRunView(
      run({
        live_state: liveState({
          agent_health: "healthy",
          agent_last_activity_at: new Date(NOW - 60_000).toISOString(),
        }),
      }),
      null,
      1,
      NOW,
      t
    );
    expect(view.title).toBe("AI agent is working on this issue");
    expect(view.detail).toContain("Last activity");
  });
});
