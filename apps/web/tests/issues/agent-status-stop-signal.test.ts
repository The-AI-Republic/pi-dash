/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { describe, expect, it, vi } from "vitest";
import type { TIssueAgentRunSummary, TIssueAgentTicker } from "@pi-dash/types";

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

import { formatNextTick, getRunView } from "../../core/components/issues/issue-detail/agent-status";

/** Substitute `{name}` placeholders, matching the app's translation shim. */
const t = ((key: string, vars?: Record<string, unknown>) =>
  vars ? key.replace(/\{(\w+)\}/g, (_m, k) => String(vars[k] ?? "")) : key) as never;

const NOW = new Date("2026-09-27T12:00:00Z").getTime();

function ticker(over: Partial<TIssueAgentTicker>): TIssueAgentTicker {
  return {
    enabled: true,
    user_disabled: false,
    used: 3,
    tick_count: 3,
    max_ticks: 10,
    interval_seconds: 10800,
    // `_stop_clock` retains next_run_at, so a disarmed ticker still carries a
    // future timestamp — exactly the shape the "Next tick 2h 57m" bug showed.
    next_run_at: new Date(NOW + 2 * 3600_000).toISOString(),
    last_tick_at: null,
    ...over,
  };
}

function run(over: Partial<TIssueAgentRunSummary>): TIssueAgentRunSummary {
  return {
    id: "run-1",
    status: "completed",
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

describe("getRunView stop_signal precedence", () => {
  it("surfaces the agent's stop on a completed run instead of a plain Done", () => {
    // The serializer always hands the newest run over as latest_run, so the
    // run view — not getTickerOnlyView — renders after a stop_signal disarm.
    const view = getRunView(run({}), ticker({ enabled: false, disarm_reason: "stop_signal" }), 6, NOW, t);
    expect(view.title).toBe("AI agent ticking stopped by the agent");
    expect(view.badge).toBe("Stopped");
    expect(view.badgeVariant).toBe("neutral");
    expect(view.detail).toContain("6 runs are done");
    expect(view.detail).toContain("no further automatic run");
  });

  it("keeps the plain completed view while the clock is still armed", () => {
    const view = getRunView(run({}), ticker({ enabled: true }), 6, NOW, t);
    expect(view.title).toBe("AI agent run completed");
    expect(view.badge).toBe("Done");
  });

  it("keeps the plain completed view for a legacy terminal_signal disarm", () => {
    const view = getRunView(run({}), ticker({ enabled: false, disarm_reason: "terminal_signal" }), 6, NOW, t);
    expect(view.title).toBe("AI agent run completed");
  });

  it("lets a failure outrank the stop — the error is the more urgent signal", () => {
    const view = getRunView(
      run({ status: "failed", error: "boom" }),
      ticker({ enabled: false, disarm_reason: "stop_signal" }),
      6,
      NOW,
      t
    );
    expect(view.title).toBe("AI agent run failed");
  });
});

describe("formatNextTick", () => {
  it("hides the countdown once the clock is disarmed, even with a stale next_run_at", () => {
    expect(formatNextTick(ticker({ enabled: false, disarm_reason: "stop_signal" }), NOW, t)).toBeNull();
    expect(formatNextTick(ticker({ enabled: false, disarm_reason: "cap_hit" }), NOW, t)).toBeNull();
  });

  it("shows the countdown while the clock is armed", () => {
    expect(formatNextTick(ticker({}), NOW, t)).toBe("2h");
  });

  it("says queued when an entry run is owed, armed or not", () => {
    expect(formatNextTick(ticker({ enabled: false, pending_entry: true }), NOW, t)).toBe("queued");
    expect(formatNextTick(ticker({ pending_entry: true }), NOW, t)).toBe("queued");
  });

  it("returns null without a ticker", () => {
    expect(formatNextTick(null, NOW, t)).toBeNull();
    expect(formatNextTick(undefined, NOW, t)).toBeNull();
  });
});
