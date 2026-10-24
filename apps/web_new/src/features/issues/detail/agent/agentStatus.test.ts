// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The agent status reads (ISS-164): every run and ticker headline, the
// budget and next-tick tiles, and the token total that folds live usage
// in without double-counting the active row.
import type { AgentLiveState, AgentRunSummary, AgentStatus, AgentTicker } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  agentPanelVisible,
  agentStatusView,
  executorLabel,
  formatAge,
  formatCountdown,
  formatNextTick,
  formatTickBudget,
  formatTokenCount,
  isActiveRunStatus,
  runStatusView,
  selectAbortableRun,
  selectAgentTicker,
  selectDisplayRun,
  selectTokenTotal,
  tickerStatusView,
} from "./agentStatus.js";

const NOW = new Date("2026-05-01T12:00:00Z").getTime();

function tickerFixture(overrides: Partial<AgentTicker> = {}): AgentTicker {
  return {
    enabled: true,
    user_disabled: false,
    used: 3,
    tick_count: 3,
    granted: 13,
    waited: 0,
    max_ticks: 13,
    remaining: 10,
    interval_seconds: 3600,
    next_run_at: "2026-05-01T14:30:00Z",
    last_tick_at: "2026-05-01T11:00:00Z",
    disarm_reason: "",
    pending_entry: false,
    can_re_tick: false,
    ...overrides,
  };
}

function liveFixture(overrides: Partial<AgentLiveState> = {}): AgentLiveState {
  return {
    observed_run_id: null,
    last_event_at: "2026-05-01T11:55:00Z",
    last_event_kind: "turn",
    last_event_summary: "Reviewing the diff",
    agent_pid: 1234,
    agent_subprocess_alive: true,
    approvals_pending: 0,
    input_tokens: 1000,
    output_tokens: 500,
    total_tokens: 1500,
    llm_model: "test-model",
    turn_count: 4,
    updated_at: "2026-05-01T11:55:00Z",
    ...overrides,
  };
}

function runFixture(overrides: Partial<AgentRunSummary> = {}): AgentRunSummary {
  return {
    id: "223e4567-e89b-12d3-a456-426614174001",
    status: "running",
    executor_kind: "local_runner",
    queue_position: null,
    runner: null,
    runner_name: "runner-1",
    created_at: "2026-05-01T11:50:00Z",
    assigned_at: "2026-05-01T11:51:00Z",
    started_at: "2026-05-01T11:52:00Z",
    ended_at: null,
    done_payload: null,
    error: "",
    error_code: "",
    error_diagnostic: null,
    llm_model: "test-model",
    input_tokens: null,
    output_tokens: null,
    total_tokens: null,
    live_state: null,
    ...overrides,
  };
}

function statusFixture(overrides: Partial<AgentStatus> = {}): AgentStatus {
  return { ticker: null, active_run: null, latest_run: null, run_count: 0, ...overrides };
}

describe("isActiveRunStatus", () => {
  it("treats in-flight states as active and terminal ones as done", () => {
    for (const status of [
      "queued",
      "assigned",
      "running",
      "cancel_requested",
      "awaiting_approval",
      "awaiting_reauth",
      "paused_awaiting_input",
    ]) {
      expect(isActiveRunStatus(status)).toBe(true);
    }
    for (const status of ["completed", "failed", "cancelled", "refused", "blocked"]) {
      expect(isActiveRunStatus(status)).toBe(false);
    }
  });
});

describe("selectDisplayRun", () => {
  it("prefers the active run, then the latest, then nothing", () => {
    const active = runFixture({ status: "running" });
    const latest = runFixture({ status: "completed" });
    expect(selectDisplayRun(statusFixture({ active_run: active, latest_run: latest }))).toBe(active);
    expect(selectDisplayRun(statusFixture({ latest_run: latest }))).toBe(latest);
    expect(selectDisplayRun(statusFixture())).toBeNull();
    expect(selectDisplayRun(null)).toBeNull();
    expect(selectDisplayRun(undefined)).toBeNull();
  });
});

describe("selectAgentTicker", () => {
  it("reads the status ticker first and the legacy field after", () => {
    const ticker = tickerFixture();
    const legacy = tickerFixture({ used: 9 });
    expect(selectAgentTicker({ agent_status: statusFixture({ ticker }), agent_ticker: legacy })).toBe(ticker);
    expect(selectAgentTicker({ agent_status: statusFixture(), agent_ticker: legacy })).toBe(legacy);
    expect(selectAgentTicker({ agent_status: statusFixture(), agent_ticker: null })).toBeNull();
    expect(selectAgentTicker({ agent_status: null, agent_ticker: null })).toBeNull();
  });
});

describe("agentPanelVisible", () => {
  it("shows with a run or a ticker and hides with neither", () => {
    expect(agentPanelVisible({ agent_status: statusFixture({ latest_run: runFixture() }), agent_ticker: null })).toBe(
      true
    );
    expect(agentPanelVisible({ agent_status: statusFixture(), agent_ticker: tickerFixture() })).toBe(true);
    expect(agentPanelVisible({ agent_status: statusFixture(), agent_ticker: null })).toBe(false);
    expect(agentPanelVisible({ agent_status: null, agent_ticker: null })).toBe(false);
  });
});

describe("selectAbortableRun", () => {
  it("offers the live run unless it is already stopping", () => {
    const active = runFixture({ status: "running" });
    expect(selectAbortableRun(statusFixture({ active_run: active }))).toBe(active);
    expect(selectAbortableRun(statusFixture({ active_run: runFixture({ status: "cancel_requested" }) }))).toBeNull();
    expect(selectAbortableRun(statusFixture({ active_run: runFixture({ status: "completed" }) }))).toBeNull();
    expect(selectAbortableRun(statusFixture())).toBeNull();
    expect(selectAbortableRun(null)).toBeNull();
  });
});

describe("formatAge", () => {
  it("renders minutes, hours, and days", () => {
    expect(formatAge("2026-05-01T11:55:00Z", NOW)).toBe("5m ago");
    expect(formatAge("2026-05-01T09:00:00Z", NOW)).toBe("3h ago");
    expect(formatAge("2026-04-28T12:00:00Z", NOW)).toBe("3d ago");
  });

  it("drops missing, invalid, and future stamps", () => {
    expect(formatAge(null, NOW)).toBeNull();
    expect(formatAge(undefined, NOW)).toBeNull();
    expect(formatAge("not-a-date", NOW)).toBeNull();
    expect(formatAge("2026-05-01T13:00:00Z", NOW)).toBeNull();
  });
});

describe("formatCountdown", () => {
  it("renders minutes, hours, and days", () => {
    expect(formatCountdown("2026-05-01T12:12:00Z", NOW)).toBe("12m");
    expect(formatCountdown("2026-05-01T14:00:00Z", NOW)).toBe("2h");
    expect(formatCountdown("2026-05-01T14:30:00Z", NOW)).toBe("2h 30m");
    expect(formatCountdown("2026-05-03T12:00:00Z", NOW)).toBe("2d");
    expect(formatCountdown("2026-05-03T15:00:00Z", NOW)).toBe("2d 3h");
  });

  it("reads reached times as due and drops bad stamps", () => {
    expect(formatCountdown("2026-05-01T11:00:00Z", NOW)).toBe("due now");
    expect(formatCountdown(null, NOW)).toBeNull();
    expect(formatCountdown(undefined, NOW)).toBeNull();
    expect(formatCountdown("not-a-date", NOW)).toBeNull();
  });
});

describe("formatTickBudget", () => {
  it("reads used-over-max with the waits beside it", () => {
    expect(formatTickBudget(tickerFixture())).toBe("3/13 runs used");
    expect(formatTickBudget(tickerFixture({ waited: 1 }))).toBe("3/13 runs used (+1 wait)");
    expect(formatTickBudget(tickerFixture({ waited: 3 }))).toBe("3/13 runs used (+3 waits)");
  });

  it("reads an uncapped pool and drops a missing ticker", () => {
    expect(formatTickBudget(tickerFixture({ max_ticks: -1 }))).toBe("3 runs used, uncapped");
    expect(formatTickBudget(null)).toBeNull();
    expect(formatTickBudget(undefined)).toBeNull();
  });
});

describe("formatNextTick", () => {
  it("reads a queued entry as queued rather than a time", () => {
    expect(formatNextTick(tickerFixture({ pending_entry: true }), NOW)).toBe("queued");
  });

  it("reads the countdown while armed and nothing once disarmed", () => {
    expect(formatNextTick(tickerFixture(), NOW)).toBe("2h 30m");
    expect(formatNextTick(tickerFixture({ enabled: false }), NOW)).toBeNull();
  });

  it("drops a missing ticker", () => {
    expect(formatNextTick(null, NOW)).toBeNull();
    expect(formatNextTick(undefined, NOW)).toBeNull();
  });
});

describe("runStatusView", () => {
  it("covers the queued branches", () => {
    const cloud = runStatusView(runFixture({ status: "queued", executor_kind: "cloud_agent" }), null, 1, NOW);
    expect(cloud).toMatchObject({ title: "Agent run queued", badge: "Queued", tone: "neutral", icon: "clock" });
    expect(cloud.detail).toContain("cloud");
    const runner = runStatusView(runFixture({ status: "queued" }), null, 1, NOW);
    expect(runner.detail).toBe("Runner runner-1");
    const bare = runStatusView(runFixture({ status: "queued", runner_name: null }), null, 1, NOW);
    expect(bare.detail).toContain("free runner");
  });

  it("covers assigned", () => {
    const view = runStatusView(runFixture({ status: "assigned" }), null, 1, NOW);
    expect(view).toMatchObject({ title: "Agent run starting", badge: "Starting", tone: "accent", icon: "spinner" });
    expect(view.detail).toBe("Runner runner-1");
  });

  it("covers the running branches", () => {
    const cloud = runStatusView(
      runFixture({ status: "running", executor_kind: "cloud_agent", live_state: liveFixture() }),
      null,
      1,
      NOW
    );
    expect(cloud).toMatchObject({ title: "Agent at work", badge: "Working", tone: "accent" });
    expect(cloud.detail).toContain("cloud agent");
    const live = runStatusView(runFixture({ status: "running", live_state: liveFixture() }), null, 1, NOW);
    expect(live.detail).toBe("Reviewing the diff (5m ago)");
    const runner = runStatusView(runFixture({ status: "running" }), null, 1, NOW);
    expect(runner.detail).toBe("Runner runner-1");
  });

  it("covers cancel_requested with and without a runner", () => {
    const named = runStatusView(runFixture({ status: "cancel_requested" }), null, 1, NOW);
    expect(named).toMatchObject({ title: "Stopping the agent run", badge: "Stopping", tone: "warning" });
    expect(named.detail).toBe("Runner runner-1");
    const bare = runStatusView(runFixture({ status: "cancel_requested", runner_name: null }), null, 1, NOW);
    expect(bare.detail).toContain("confirming the stop");
  });

  it("covers awaiting_approval with and without live detail", () => {
    const live = runStatusView(runFixture({ status: "awaiting_approval", live_state: liveFixture() }), null, 1, NOW);
    expect(live).toMatchObject({ title: "Agent run needs approval", badge: "Approval", tone: "warning" });
    expect(live.detail).toBe("Reviewing the diff (5m ago)");
    const runner = runStatusView(runFixture({ status: "awaiting_approval" }), null, 1, NOW);
    expect(runner.detail).toBe("Runner runner-1");
  });

  it("covers awaiting_reauth", () => {
    const view = runStatusView(runFixture({ status: "awaiting_reauth" }), null, 1, NOW);
    expect(view).toMatchObject({ title: "Agent run needs fresh credentials", badge: "Re-auth", tone: "warning" });
    expect(view.detail).toBe("Runner runner-1");
  });

  it("covers paused_awaiting_input with a question, a summary, and neither", () => {
    const question = runStatusView(
      runFixture({ status: "paused_awaiting_input", done_payload: { autonomy: { question_for_human: "Which API?" } } }),
      null,
      1,
      NOW
    );
    expect(question).toMatchObject({ title: "Agent run paused for input", badge: "Paused" });
    expect(question.detail).toBe("Which API?");
    const summary = runStatusView(
      runFixture({ status: "paused_awaiting_input", done_payload: { summary: "Waiting on scope" } }),
      null,
      1,
      NOW
    );
    expect(summary.detail).toBe("Waiting on scope");
    const bare = runStatusView(runFixture({ status: "paused_awaiting_input" }), null, 1, NOW);
    expect(bare.detail).toBe("Runner runner-1");
  });

  it("covers blocked with blockers, a reason, a summary, and none", () => {
    const list = runStatusView(
      runFixture({ status: "blocked", done_payload: { blockers: ["token", "scope"] } }),
      null,
      1,
      NOW
    );
    expect(list).toMatchObject({ title: "Agent run blocked", badge: "Blocked" });
    expect(list.detail).toBe("token, scope");
    const reason = runStatusView(
      runFixture({ status: "blocked", done_payload: { autonomy: { reason: "no access" } } }),
      null,
      1,
      NOW
    );
    expect(reason.detail).toBe("no access");
    const summary = runStatusView(runFixture({ status: "blocked", done_payload: { summary: "stuck" } }), null, 1, NOW);
    expect(summary.detail).toBe("stuck");
    const bare = runStatusView(runFixture({ status: "blocked" }), null, 1, NOW);
    expect(bare.detail).toBe("Runner runner-1");
  });

  it("covers failed with a diagnostic, error text, and neither", () => {
    const diagnosed = runStatusView(
      runFixture({
        status: "failed",
        error_diagnostic: {
          source: "agent",
          source_label: "Agent",
          kind: "timeout",
          summary: "The run timed out",
          action: "retry",
        },
      }),
      null,
      1,
      NOW
    );
    expect(diagnosed).toMatchObject({ title: "Agent run failed", badge: "Failed", tone: "danger" });
    expect(diagnosed.detail).toBe("Agent: The run timed out");
    const text = runStatusView(runFixture({ status: "failed", error: "boom" }), null, 1, NOW);
    expect(text.detail).toBe("boom");
    const bare = runStatusView(runFixture({ status: "failed" }), null, 1, NOW);
    expect(bare.detail).toContain("did not finish");
  });

  it("covers cancelled with and without an end stamp", () => {
    const ended = runStatusView(runFixture({ status: "cancelled", ended_at: "2026-05-01T11:00:00Z" }), null, 1, NOW);
    expect(ended).toMatchObject({ title: "Agent run cancelled", badge: "Cancelled" });
    expect(ended.detail).toBe("1h ago");
    const bare = runStatusView(runFixture({ status: "cancelled" }), null, 1, NOW);
    expect(bare.detail).toBeNull();
  });

  it("covers refused with and without error text", () => {
    const text = runStatusView(runFixture({ status: "refused", error: "no capacity" }), null, 1, NOW);
    expect(text).toMatchObject({ title: "Agent run refused", badge: "Refused", tone: "danger" });
    expect(text.detail).toBe("no capacity");
    const bare = runStatusView(runFixture({ status: "refused" }), null, 1, NOW);
    expect(bare.detail).toContain("declined");
  });

  it("covers completed with a ticking clock, a quiet clock, and a stop", () => {
    const ticking = runStatusView(runFixture({ status: "completed" }), tickerFixture(), 4, NOW);
    expect(ticking).toMatchObject({ title: "Agent run finished", badge: "Done", tone: "success" });
    expect(ticking.detail).toBe("4 runs finished, next run 2h 30m");
    const quiet = runStatusView(
      runFixture({ status: "completed" }),
      tickerFixture({ enabled: false, disarm_reason: "cap_hit" }),
      4,
      NOW
    );
    expect(quiet.detail).toBe("4 runs finished");
    const stopped = runStatusView(
      runFixture({ status: "completed" }),
      tickerFixture({ enabled: false, disarm_reason: "stop_signal" }),
      4,
      NOW
    );
    expect(stopped).toMatchObject({ title: "Agent ticking stopped", badge: "Stopped" });
    expect(stopped.detail).toContain("4 runs finished");
    const armedBare = runStatusView(runFixture({ status: "completed" }), tickerFixture({ next_run_at: null }), 4, NOW);
    expect(armedBare.detail).toBe("4 runs finished");
    const single = runStatusView(runFixture({ status: "completed" }), null, 1, NOW);
    expect(single.detail).toBe("1 run finished");
    const empty = runStatusView(runFixture({ status: "completed" }), null, 0, NOW);
    expect(empty.detail).toBe("1 run finished");
  });

  it("renders a retired status neutrally with its raw token", () => {
    const named = runStatusView(runFixture({ status: "waiting_for_worktree" }), null, 1, NOW);
    expect(named).toMatchObject({ title: "Agent run in a retired state", badge: "waiting_for_worktree" });
    expect(named.detail).toBe("Runner runner-1");
    const bare = runStatusView(runFixture({ status: "waiting_for_worktree", runner_name: null }), null, 1, NOW);
    expect(bare.detail).toBe("Reported status: waiting_for_worktree");
  });

  it("covers live detail with only a summary, only an age, and neither", () => {
    const summaryOnly = runStatusView(
      runFixture({ status: "running", live_state: liveFixture({ last_event_at: null }) }),
      null,
      1,
      NOW
    );
    expect(summaryOnly.detail).toBe("Reviewing the diff");
    const ageOnly = runStatusView(
      runFixture({ status: "running", live_state: liveFixture({ last_event_summary: null }) }),
      null,
      1,
      NOW
    );
    expect(ageOnly.detail).toBe("Latest activity 5m ago");
    const blank = runStatusView(
      runFixture({ status: "running", live_state: liveFixture({ last_event_at: null, last_event_summary: " " }) }),
      null,
      1,
      NOW
    );
    expect(blank.detail).toBe("Runner runner-1");
  });

  it("truncates long detail lines", () => {
    const view = runStatusView(runFixture({ status: "failed", error: `x${"y".repeat(200)}` }), null, 1, NOW);
    expect(view.detail?.length).toBe(140);
    expect(view.detail?.endsWith("...")).toBe(true);
  });
});

describe("tickerStatusView", () => {
  it("covers the scheduled branches", () => {
    const timed = tickerStatusView(tickerFixture(), NOW);
    expect(timed).toMatchObject({ title: "Agent ticking scheduled", badge: "Scheduled" });
    expect(timed.detail).toBe("Next run 2h 30m");
    const bare = tickerStatusView(tickerFixture({ next_run_at: null }), NOW);
    expect(bare.detail).toContain("next scheduled run");
  });

  it("covers the disarmed branches", () => {
    const switched = tickerStatusView(tickerFixture({ enabled: false, user_disabled: true }), NOW);
    expect(switched).toMatchObject({ title: "Agent ticking switched off", badge: "Disabled" });
    const reasoned = tickerStatusView(tickerFixture({ enabled: false, disarm_reason: "user_disabled" }), NOW);
    expect(reasoned.title).toBe("Agent ticking switched off");
    const stopped = tickerStatusView(tickerFixture({ enabled: false, disarm_reason: "stop_signal" }), NOW);
    expect(stopped).toMatchObject({ title: "Agent ticking stopped", badge: "Stopped" });
    const capped = tickerStatusView(tickerFixture({ enabled: false, disarm_reason: "cap_hit" }), NOW);
    expect(capped).toMatchObject({ title: "Agent run budget spent", badge: "Limit" });
    expect(capped.detail).toBe("3/13 runs used");
    const spent = tickerStatusView(tickerFixture({ enabled: false, disarm_reason: "pool_spent" }), NOW);
    expect(spent.title).toBe("Agent run budget spent");
    const off = tickerStatusView(tickerFixture({ enabled: false, disarm_reason: "other" }), NOW);
    expect(off).toMatchObject({ title: "Agent ticking off", badge: "Off" });
    expect(off.detail).toBe("3/13 runs used");
  });
});

describe("agentStatusView", () => {
  it("prefers the run view, then the ticker view, then nothing", () => {
    const run = runFixture({ status: "running" });
    const ticker = tickerFixture();
    const withRun = agentStatusView(
      { agent_status: statusFixture({ active_run: run, ticker, run_count: 2 }), agent_ticker: null },
      NOW
    );
    expect(withRun?.title).toBe("Agent at work");
    const tickerOnly = agentStatusView({ agent_status: statusFixture(), agent_ticker: ticker }, NOW);
    expect(tickerOnly?.title).toBe("Agent ticking scheduled");
    const none = agentStatusView({ agent_status: null, agent_ticker: null }, NOW);
    expect(none).toBeNull();
  });
});

describe("executorLabel", () => {
  it("names the cloud agent and the local runner", () => {
    expect(executorLabel("cloud_agent")).toBe("Cloud agent");
    expect(executorLabel("local_runner")).toBe("Local runner");
    expect(executorLabel("other")).toBe("Local runner");
  });
});

describe("selectTokenTotal", () => {
  it("hides the tile only with no figure at all", () => {
    expect(selectTokenTotal(statusFixture())).toBeNull();
    expect(selectTokenTotal(null)).toBeNull();
    expect(selectTokenTotal(undefined)).toBeNull();
  });

  it("reads the stored sum alone", () => {
    expect(selectTokenTotal(statusFixture({ total_tokens: 1200 }))).toBe(1200);
  });

  it("shows a reported zero instead of hiding", () => {
    expect(selectTokenTotal(statusFixture({ total_tokens: 0 }))).toBe(0);
  });

  it("adds the whole live figure while the row is unwritten", () => {
    const status = statusFixture({
      total_tokens: 1000,
      active_run: runFixture({ total_tokens: null, live_state: liveFixture({ total_tokens: 400 }) }),
    });
    expect(selectTokenTotal(status)).toBe(1400);
  });

  it("adds just the delta while the row lags", () => {
    const status = statusFixture({
      total_tokens: 1000,
      active_run: runFixture({ total_tokens: 300, live_state: liveFixture({ total_tokens: 500 }) }),
    });
    expect(selectTokenTotal(status)).toBe(1200);
  });

  it("adds nothing once the row caught up", () => {
    const status = statusFixture({
      total_tokens: 1000,
      active_run: runFixture({ total_tokens: 500, live_state: liveFixture({ total_tokens: 500 }) }),
    });
    expect(selectTokenTotal(status)).toBe(1000);
  });

  it("reads live usage without a stored sum", () => {
    const status = statusFixture({
      active_run: runFixture({ total_tokens: null, live_state: liveFixture({ total_tokens: 250 }) }),
    });
    expect(selectTokenTotal(status)).toBe(250);
  });

  it("hides a lone row figure with no stored sum and no live usage", () => {
    const status = statusFixture({
      active_run: runFixture({ total_tokens: 700, live_state: null }),
    });
    expect(selectTokenTotal(status)).toBeNull();
  });
});

describe("formatTokenCount", () => {
  it("abbreviates thousands and millions", () => {
    expect(formatTokenCount(0)).toBe("0");
    expect(formatTokenCount(999)).toBe("999");
    expect(formatTokenCount(12_800)).toBe("12.8k");
    expect(formatTokenCount(1_200_000)).toBe("1.2M");
  });
});

describe("payload readers", () => {
  it("drops non-object and empty payloads", () => {
    for (const payload of [null, undefined, "text", 42, ["a"]]) {
      const paused = runStatusView(
        runFixture({ status: "paused_awaiting_input", done_payload: payload }),
        null,
        1,
        NOW
      );
      expect(paused.detail).toBe("Runner runner-1");
      const blocked = runStatusView(runFixture({ status: "blocked", done_payload: payload }), null, 1, NOW);
      expect(blocked.detail).toBe("Runner runner-1");
    }
    const empty = runStatusView(
      runFixture({ status: "blocked", done_payload: { blockers: ["  ", 7], summary: "  " } }),
      null,
      1,
      NOW
    );
    expect(empty.detail).toBe("Runner runner-1");
    const mistyped = runStatusView(
      runFixture({ status: "paused_awaiting_input", done_payload: { summary: 7 } }),
      null,
      1,
      NOW
    );
    expect(mistyped.detail).toBe("Runner runner-1");
    const nested = runStatusView(
      runFixture({ status: "paused_awaiting_input", done_payload: { autonomy: ["x"] } }),
      null,
      1,
      NOW
    );
    expect(nested.detail).toBe("Runner runner-1");
  });
});
