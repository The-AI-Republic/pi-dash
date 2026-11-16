/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * The agent activity badge must not call a quiet run stalled when its runner
 * runs the passive-kill watchdog: a few silent minutes there is a long tool
 * call, and only the runner's own ``unhealthy`` verdict is a problem. Older
 * runners (no ``agent_health``) keep the short "stalled" heuristic.
 */

import { describe, expect, it } from "vitest";
import type { IRunnerLiveState } from "@pi-dash/types";
import { deriveActivityBadge } from "@/components/runners/runner-agent-status-panel";

const NOW = Date.parse("2026-10-19T12:00:00Z");
const minutesAgo = (m: number) => new Date(NOW - m * 60_000).toISOString();

function liveState(overrides: Partial<IRunnerLiveState>): IRunnerLiveState {
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
    updated_at: minutesAgo(0),
    ...overrides,
  };
}

describe("deriveActivityBadge", () => {
  it("reads a quiet run on a watchdog runner as quiet, not stalled", () => {
    const state = liveState({ agent_health: "healthy", last_event_at: minutesAgo(20) });
    expect(deriveActivityBadge(state, NOW)).toBe("quiet");
  });

  it("keeps the stalled heuristic for runners that predate the watchdog", () => {
    const state = liveState({ last_event_at: minutesAgo(20) });
    expect(deriveActivityBadge(state, NOW)).toBe("stalled");
  });

  it("shows the runner's unhealthy verdict", () => {
    const state = liveState({ agent_health: "unhealthy", agent_last_activity_at: minutesAgo(25 * 60) });
    expect(deriveActivityBadge(state, NOW)).toBe("unhealthy");
  });

  it("falls back to the watchdog's last activity when observability is off", () => {
    expect(
      deriveActivityBadge(liveState({ agent_health: "healthy", agent_last_activity_at: minutesAgo(0) }), NOW)
    ).toBe("active");
    expect(deriveActivityBadge(liveState({ agent_health: "healthy" }), NOW)).toBe("unknown");
  });

  it("still ranks a dead subprocess and a pending approval above health", () => {
    expect(deriveActivityBadge(liveState({ agent_health: "unhealthy", agent_subprocess_alive: false }), NOW)).toBe(
      "dead"
    );
    expect(deriveActivityBadge(liveState({ agent_health: "unhealthy", approvals_pending: 1 }), NOW)).toBe(
      "awaiting_approval"
    );
  });
});
