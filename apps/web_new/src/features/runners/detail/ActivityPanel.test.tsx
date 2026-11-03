// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { RunnerLiveState } from "@pidash/api-client";
import { act, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ActivityPanel } from "./ActivityPanel.js";

const NOW = Date.parse("2026-01-01T12:00:00.000Z");

function live(overrides: Partial<RunnerLiveState> = {}): RunnerLiveState {
  return {
    observed_run_id: null,
    last_event_at: new Date(NOW - 5_000).toISOString(),
    last_event_kind: "parity-probe-kind",
    last_event_summary: "parity probe summary",
    agent_pid: 4242,
    agent_subprocess_alive: true,
    approvals_pending: 3,
    input_tokens: 1500,
    output_tokens: 2_500_000,
    total_tokens: 2_501_500,
    usage: {},
    llm_model: "parity-model-9",
    turn_count: 7,
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function gridValues(): Record<string, string> {
  const section = screen.getByRole("region", { name: "Agent activity" });
  const terms = Array.from(section.querySelectorAll("dt"));
  const out: Record<string, string> = {};
  for (const term of terms) {
    out[(term.textContent ?? "").trim()] = (term.nextElementSibling?.textContent ?? "").trim();
  }
  return out;
}

afterEach(() => {
  vi.useRealTimers();
});

describe("ActivityPanel", () => {
  it("renders the heading, badge and every telemetry scalar", () => {
    render(<ActivityPanel runnerName="alpha" liveState={live()} now={() => NOW} />);
    const region = screen.getByRole("region", { name: "Agent activity" });
    expect(within(region).getByRole("heading", { name: "Agent" })).toBeInTheDocument();
    expect(within(region).getByText("Awaiting approval")).toBeInTheDocument();
    expect(within(region).getByText("alpha")).toBeInTheDocument();
    expect(gridValues()).toEqual({
      "Last activity": "5s ago",
      "Last event": "parity-probe-kind",
      "Agent PID": "4242",
      "Subprocess alive": "yes",
      Approvals: "3",
      Tokens: "2.5M",
      Model: "parity-model-9",
      Turn: "7",
    });
  });

  it("renders dash fallbacks without a snapshot", () => {
    render(<ActivityPanel runnerName="alpha" liveState={null} now={() => NOW} />);
    const region = screen.getByRole("region", { name: "Agent activity" });
    expect(within(region).getByText("Unknown")).toBeInTheDocument();
    expect(gridValues()).toEqual({
      "Last activity": "—",
      "Last event": "—",
      "Agent PID": "—",
      "Subprocess alive": "—",
      Approvals: "0",
      Tokens: "—",
      Model: "—",
      Turn: "—",
    });
  });

  it("renders the dead badge and a no liveness value", () => {
    render(
      <ActivityPanel
        runnerName="alpha"
        liveState={live({ agent_subprocess_alive: false, approvals_pending: 0 })}
        now={() => NOW}
      />
    );
    expect(screen.getByText("Subprocess dead")).toBeInTheDocument();
    expect(gridValues()["Subprocess alive"]).toBe("no");
  });

  it("ages on its own tick without a new snapshot", () => {
    vi.useFakeTimers();
    let now = NOW;
    render(
      <ActivityPanel runnerName="alpha" liveState={live({ approvals_pending: 0 })} tickMs={100} now={() => now} />
    );
    expect(gridValues()["Last activity"]).toBe("5s ago");
    now = NOW + 60_000;
    act(() => {
      vi.advanceTimersByTime(100);
    });
    expect(gridValues()["Last activity"]).toBe("1m ago");
    expect(screen.getByText("Thinking")).toBeInTheDocument();
  });

  it("shows the summary as a tooltip on the last-activity cell", () => {
    render(<ActivityPanel runnerName="alpha" liveState={live()} now={() => NOW} />);
    const section = screen.getByRole("region", { name: "Agent activity" });
    expect(within(section).getByText("5s ago")).toHaveAttribute("title", "parity probe summary");
  });
});
