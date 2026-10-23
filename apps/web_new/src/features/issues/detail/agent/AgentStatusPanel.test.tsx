// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The agent status panel (ISS-164): headline plus stats tiles only with
// a run or a ticker behind it, Re-tick and Abort run through the runners
// dispatch hooks, the fifteen-second live poll, and the idle refresh at
// the next tick.
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { act } from "react";
import type { AgentLiveState, AgentRunSummary, AgentStatus, AgentTicker, Issue } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  reTick: vi.fn(),
  reTicking: false,
  abort: vi.fn(),
  aborting: false,
}));

vi.mock("../../../runners/index.js", () => ({
  useReTick: () => ({ reTick: harness.reTick, isSubmitting: harness.reTicking }),
  useAbortAgentRun: () => ({ mutateAsync: harness.abort, isPending: harness.aborting }),
}));

const { AgentStatusPanel } = await import("./AgentStatusPanel.js");

const NOW = new Date("2026-05-01T12:00:00Z").getTime();
const ISSUE_ID = "123e4567-e89b-12d3-a456-426614174000";

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

function issueFixture(overrides: Partial<Issue> = {}): Issue {
  return {
    id: ISSUE_ID,
    name: "Fix login",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    sequence_id: 42,
    project_id: "223e4567-e89b-12d3-a456-426614174002",
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p>Words</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(NOW);
  harness.reTick.mockReset();
  harness.abort.mockReset();
  harness.reTicking = false;
  harness.aborting = false;
  harness.abort.mockResolvedValue({ id: "run-1" });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function renderPanel(issue: Issue, refresh: () => Promise<unknown> = async () => undefined) {
  return render(<AgentStatusPanel workspaceSlug="acme" issue={issue} refresh={refresh} />);
}

describe("presence", () => {
  it("renders nothing without a run or a ticker", () => {
    renderPanel(issueFixture());
    expect(screen.queryByTestId("agent-status-panel")).toBeNull();
    expect(screen.queryByRole("button", { name: /re-tick/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /abort run/i })).toBeNull();
  });

  it("renders the headline, badge, and detail for a live run", () => {
    renderPanel(
      issueFixture({
        agent_status: statusFixture({
          active_run: runFixture({ live_state: liveFixture() }),
          latest_run: runFixture({ status: "completed" }),
          run_count: 2,
          ticker: tickerFixture(),
        }),
      })
    );
    expect(screen.getByTestId("agent-status-panel")).toHaveAccessibleName("Agent status");
    expect(screen.getByText("Agent at work")).toBeInTheDocument();
    expect(screen.getByText("Working")).toBeInTheDocument();
    expect(screen.getByText("Reviewing the diff (5m ago)")).toBeInTheDocument();
  });

  it("renders a ticker clock with no run behind it", () => {
    renderPanel(issueFixture({ agent_status: null, agent_ticker: tickerFixture() }));
    expect(screen.getByText("Agent ticking scheduled")).toBeInTheDocument();
    expect(screen.getByText("Scheduled")).toBeInTheDocument();
  });

  it("omits the detail line when the view has none", () => {
    renderPanel(
      issueFixture({
        agent_status: statusFixture({ latest_run: runFixture({ status: "cancelled", ended_at: null }) }),
      })
    );
    expect(screen.getByText("Agent run cancelled")).toBeInTheDocument();
    const panel = screen.getByTestId("agent-status-panel");
    expect(panel.querySelectorAll("p").length).toBe(1);
  });
});

describe("tiles", () => {
  it("shows runs, executor, next tick, budget, and tokens", () => {
    renderPanel(
      issueFixture({
        agent_status: statusFixture({
          latest_run: runFixture({ status: "completed", executor_kind: "cloud_agent" }),
          run_count: 2,
          ticker: tickerFixture(),
          total_tokens: 12_800,
        }),
      })
    );
    expect(screen.getByText("Runs")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
    expect(screen.getByText("Cloud agent")).toBeInTheDocument();
    expect(screen.getByText("2h 30m")).toBeInTheDocument();
    expect(screen.getByText("3/13 runs used")).toBeInTheDocument();
    expect(screen.getByText("12.8k")).toBeInTheDocument();
  });

  it("drops the whole grid when no tile has a figure", () => {
    renderPanel(issueFixture({ agent_status: statusFixture({ active_run: runFixture(), run_count: 0 }) }));
    expect(screen.getByText("Agent at work")).toBeInTheDocument();
    expect(screen.queryByText("Runs")).toBeNull();
    expect(screen.queryByText("Executor")).toBeNull();
    expect(screen.queryByText("Next tick")).toBeNull();
    expect(screen.queryByText("Budget")).toBeNull();
    expect(screen.queryByText("Tokens")).toBeNull();
  });

  it("drops tiles one by one as their figures go missing", () => {
    renderPanel(
      issueFixture({
        agent_status: statusFixture({
          latest_run: runFixture({ status: "completed" }),
          run_count: 1,
          ticker: tickerFixture({ enabled: false, disarm_reason: "other" }),
        }),
      })
    );
    // A disarmed clock keeps its stamp but shows no next tick; with no
    // stored sum and no live usage the token tile hides too.
    expect(screen.queryByText("Next tick")).toBeNull();
    expect(screen.queryByText("Tokens")).toBeNull();
    expect(screen.getByText("Budget")).toBeInTheDocument();
    expect(screen.getByText("Runs")).toBeInTheDocument();
    expect(screen.getByText("Local runner")).toBeInTheDocument();
  });
});

describe("re-tick", () => {
  it("asks for fresh budget through the runners hook", () => {
    renderPanel(issueFixture({ agent_status: statusFixture({ ticker: tickerFixture({ can_re_tick: true }) }) }));
    fireEvent.click(screen.getByRole("button", { name: "Re-tick" }));
    expect(harness.reTick).toHaveBeenCalledWith(ISSUE_ID);
  });

  it("hides while the server offers no refill and disables while asking", () => {
    const { rerender } = renderPanel(issueFixture({ agent_status: statusFixture({ ticker: tickerFixture() }) }));
    expect(screen.queryByRole("button", { name: "Re-tick" })).toBeNull();
    harness.reTicking = true;
    rerender(
      <AgentStatusPanel
        workspaceSlug="acme"
        issue={issueFixture({ agent_status: statusFixture({ ticker: tickerFixture({ can_re_tick: true }) }) })}
        refresh={async () => undefined}
      />
    );
    expect(screen.getByRole("button", { name: "Re-tick" })).toBeDisabled();
  });
});

describe("abort", () => {
  it("confirms, then stops the run and closes on success", async () => {
    renderPanel(
      issueFixture({
        agent_status: statusFixture({ active_run: runFixture(), latest_run: runFixture() }),
      })
    );
    fireEvent.click(screen.getByRole("button", { name: "Abort run" }));
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByText("Abort run?")).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Abort run" }));
    expect(harness.abort).toHaveBeenCalledWith({ runId: "223e4567-e89b-12d3-a456-426614174001" });
    await act(async () => {});
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("keeps the dialog open when the stop is refused", async () => {
    harness.abort.mockRejectedValue(new Error("offline"));
    renderPanel(issueFixture({ agent_status: statusFixture({ active_run: runFixture() }) }));
    fireEvent.click(screen.getByRole("button", { name: "Abort run" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Abort run" }));
    await act(async () => {});
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("closes without a write when the run landed while open", async () => {
    const issue = issueFixture({ agent_status: statusFixture({ active_run: runFixture() }) });
    const { rerender } = renderPanel(issue);
    fireEvent.click(screen.getByRole("button", { name: "Abort run" }));
    rerender(
      <AgentStatusPanel
        workspaceSlug="acme"
        issue={issueFixture({
          agent_status: statusFixture({ latest_run: runFixture({ status: "completed" }) }),
        })}
        refresh={async () => undefined}
      />
    );
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Abort run" }));
    expect(harness.abort).not.toHaveBeenCalled();
    await act(async () => {});
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("cancels and dismisses without a write", () => {
    renderPanel(issueFixture({ agent_status: statusFixture({ active_run: runFixture() }) }));
    fireEvent.click(screen.getByRole("button", { name: "Abort run" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }));
    expect(harness.abort).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("dismisses through the dialog close control", () => {
    renderPanel(issueFixture({ agent_status: statusFixture({ active_run: runFixture() }) }));
    fireEvent.click(screen.getByRole("button", { name: "Abort run" }));
    const dialog = screen.getByRole("dialog");
    const close = within(dialog).getByRole("button", { name: /close/i });
    fireEvent.click(close);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("reads as aborting while the stop is out", () => {
    const issue = issueFixture({ agent_status: statusFixture({ active_run: runFixture() }) });
    const { rerender } = renderPanel(issue);
    fireEvent.click(screen.getByRole("button", { name: "Abort run" }));
    harness.aborting = true;
    rerender(<AgentStatusPanel workspaceSlug="acme" issue={issue} refresh={async () => undefined} />);
    // The open dialog hides the trigger behind it, so read it hidden.
    expect(screen.getAllByRole("button", { name: "Abort run", hidden: true })[0]).toBeDisabled();
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByRole("button", { name: "Aborting…" })).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeDisabled();
  });

  it("hides while nothing can be stopped", () => {
    const stopping = renderPanel(
      issueFixture({
        agent_status: statusFixture({ active_run: runFixture({ status: "cancel_requested" }) }),
      })
    );
    expect(screen.queryByRole("button", { name: "Abort run" })).toBeNull();
    stopping.unmount();
    renderPanel(
      issueFixture({
        agent_status: statusFixture({ latest_run: runFixture({ status: "completed" }) }),
      })
    );
    expect(screen.queryByRole("button", { name: "Abort run" })).toBeNull();
  });
});

describe("refresh cadence", () => {
  it("polls every fifteen seconds while a run is in flight", () => {
    const refresh = vi.fn(async () => undefined);
    renderPanel(
      issueFixture({
        agent_status: statusFixture({
          active_run: runFixture(),
          ticker: tickerFixture({ next_run_at: "2026-05-01T12:00:01Z" }),
        }),
      }),
      refresh
    );
    act(() => {
      vi.advanceTimersByTime(16_000);
    });
    // The poll fired once; the scheduled next-tick refresh stays off
    // while a run is in flight even though the tick just passed.
    expect(refresh).toHaveBeenCalledTimes(1);
    act(() => {
      vi.advanceTimersByTime(30_000);
    });
    expect(refresh).toHaveBeenCalledTimes(3);
  });

  it("leaves an idle issue alone until its next tick", () => {
    const refresh = vi.fn(async () => undefined);
    renderPanel(
      issueFixture({
        agent_status: statusFixture({
          latest_run: runFixture({ status: "completed" }),
          ticker: tickerFixture({ next_run_at: "2026-05-01T12:10:00Z" }),
        }),
      }),
      refresh
    );
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(refresh).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(9 * 60_000);
    });
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("schedules nothing without an armed clock stamp", () => {
    for (const ticker of [
      tickerFixture({ enabled: false }),
      tickerFixture({ next_run_at: null }),
      tickerFixture({ next_run_at: "not-a-date" }),
    ]) {
      const refresh = vi.fn(async () => undefined);
      const view = renderPanel(
        issueFixture({
          agent_status: statusFixture({ latest_run: runFixture({ status: "completed" }), ticker }),
        }),
        refresh
      );
      act(() => {
        vi.advanceTimersByTime(3_600_000);
      });
      expect(refresh).not.toHaveBeenCalled();
      view.unmount();
    }
  });

  it("re-renders the relative labels as the clock ticks", () => {
    const refresh = vi.fn(async () => undefined);
    renderPanel(
      issueFixture({
        agent_status: statusFixture({
          latest_run: runFixture({ status: "completed" }),
          ticker: tickerFixture({ next_run_at: "2026-05-01T13:01:00Z" }),
        }),
      }),
      refresh
    );
    expect(screen.getByText("1h 1m")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(120_000);
    });
    expect(screen.getByText("59m")).toBeInTheDocument();
    expect(refresh).not.toHaveBeenCalled();
  });
});
