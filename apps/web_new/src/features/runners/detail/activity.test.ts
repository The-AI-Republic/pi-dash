// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { RunnerLiveState } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  ACTIVITY_LABELS,
  ACTIVE_WINDOW_MS,
  THINKING_WINDOW_MS,
  activityBadge,
  compactCount,
  livenessLabel,
  relativeAge,
} from "./activity.js";

const NOW = Date.parse("2026-01-01T12:00:00.000Z");

function state(overrides: Partial<RunnerLiveState> = {}): RunnerLiveState {
  return {
    observed_run_id: null,
    last_event_at: new Date(NOW - 1_000).toISOString(),
    last_event_kind: "tool",
    last_event_summary: "summary",
    agent_pid: 42,
    agent_subprocess_alive: true,
    approvals_pending: 0,
    input_tokens: 1,
    output_tokens: 2,
    total_tokens: 3,
    usage: {},
    llm_model: "model",
    turn_count: 1,
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

describe("activityBadge", () => {
  it("is unknown without a snapshot", () => {
    expect(activityBadge(null, NOW)).toBe("unknown");
    expect(activityBadge(undefined, NOW)).toBe("unknown");
  });

  it("prefers a dead subprocess over everything else", () => {
    expect(activityBadge(state({ agent_subprocess_alive: false, approvals_pending: 3 }), NOW)).toBe("dead");
  });

  it("prefers pending approvals over freshness", () => {
    expect(activityBadge(state({ approvals_pending: 2 }), NOW)).toBe("awaiting_approval");
  });

  it("treats a missing approvals count as none", () => {
    expect(activityBadge(state({ approvals_pending: null }), NOW)).toBe("active");
  });

  it("is unknown when the event stamp is missing or unusable", () => {
    expect(activityBadge(state({ last_event_at: null }), NOW)).toBe("unknown");
    expect(activityBadge(state({ last_event_at: "not-a-date" }), NOW)).toBe("unknown");
    expect(activityBadge(state({ last_event_at: new Date(NOW + 60_000).toISOString() }), NOW)).toBe("unknown");
  });

  it("ages from active through thinking to stalled at the thresholds", () => {
    expect(activityBadge(state({ last_event_at: new Date(NOW - ACTIVE_WINDOW_MS).toISOString() }), NOW)).toBe("active");
    expect(activityBadge(state({ last_event_at: new Date(NOW - ACTIVE_WINDOW_MS - 1).toISOString() }), NOW)).toBe(
      "thinking"
    );
    expect(activityBadge(state({ last_event_at: new Date(NOW - THINKING_WINDOW_MS).toISOString() }), NOW)).toBe(
      "thinking"
    );
    expect(activityBadge(state({ last_event_at: new Date(NOW - THINKING_WINDOW_MS - 1).toISOString() }), NOW)).toBe(
      "stalled"
    );
  });

  it("names every badge", () => {
    expect(Object.keys(ACTIVITY_LABELS)).toHaveLength(6);
  });
});

describe("relativeAge", () => {
  it("renders seconds, minutes and hours", () => {
    expect(relativeAge(new Date(NOW - 5_000).toISOString(), NOW)).toBe("5s ago");
    expect(relativeAge(new Date(NOW - 120_000).toISOString(), NOW)).toBe("2m ago");
    expect(relativeAge(new Date(NOW - 7_200_000).toISOString(), NOW)).toBe("2h ago");
  });

  it("falls back to a dash for missing, invalid or future stamps", () => {
    expect(relativeAge(null, NOW)).toBe("—");
    expect(relativeAge("nope", NOW)).toBe("—");
    expect(relativeAge(new Date(NOW + 1_000).toISOString(), NOW)).toBe("—");
  });
});

describe("compactCount", () => {
  it("formats plain, thousands and millions", () => {
    expect(compactCount(null)).toBe("—");
    expect(compactCount(undefined)).toBe("—");
    expect(compactCount(999)).toBe("999");
    expect(compactCount(1_500)).toBe("1.5k");
    expect(compactCount(2_501_500)).toBe("2.5M");
  });
});

describe("livenessLabel", () => {
  it("maps the three-way flag", () => {
    expect(livenessLabel(true)).toBe("yes");
    expect(livenessLabel(false)).toBe("no");
    expect(livenessLabel(null)).toBe("—");
  });
});
