// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner model catalog page helper tests (NEWPIDASH-169).
import { describe, expect, it } from "vitest";
import type { RunnerModelRow } from "@pidash/api-client";

import {
  RUNNER_MODEL_AGENTS,
  RUNNER_MODEL_EFFORTS,
  formatEfforts,
  nextSortOrder,
  rowsForAgent,
  swapSortOrders,
} from "./runnerModels.js";

function row(partial: Partial<RunnerModelRow>): RunnerModelRow {
  return {
    id: partial.id ?? "11111111-1111-4111-8111-111111111111",
    agent: partial.agent ?? "codex",
    model: partial.model ?? "gpt-5.5",
    label: partial.label ?? "GPT-5.5",
    reasoning_efforts: partial.reasoning_efforts ?? [],
    sort_order: partial.sort_order ?? 0,
    is_enabled: partial.is_enabled ?? true,
    is_preselected: partial.is_preselected ?? false,
    created_at: partial.created_at ?? "2026-01-01T00:00:00Z",
    updated_at: partial.updated_at ?? "2026-01-01T00:00:00Z",
  };
}

describe("agent sections", () => {
  it("covers the six agents in picker order", () => {
    expect(RUNNER_MODEL_AGENTS.map((entry) => entry.agent)).toEqual([
      "claude-code",
      "codex",
      "cursor-agent",
      "open-claw",
      "grok",
      "muse-code",
    ]);
    expect(RUNNER_MODEL_EFFORTS).toEqual(["low", "medium", "high", "xhigh"]);
  });

  it("filters and orders one agent's rows", () => {
    const rows = [
      row({ model: "b", sort_order: 2 }),
      row({ model: "a", sort_order: 1 }),
      row({ model: "opus", agent: "claude-code", sort_order: 0 }),
    ];
    expect(rowsForAgent(rows, "codex").map((entry) => entry.model)).toEqual(["a", "b"]);
    expect(rowsForAgent(rows, "grok")).toEqual([]);
  });
});

describe("formatting and reorder maths", () => {
  it("formats efforts or an em dash", () => {
    expect(formatEfforts(row({ reasoning_efforts: ["low", "xhigh"] }))).toBe("Low, Extra High");
    expect(formatEfforts(row({ reasoning_efforts: [] }))).toBe("—");
  });

  it("appends after the last row", () => {
    expect(nextSortOrder([])).toBe(0);
    expect(nextSortOrder([row({ sort_order: 3 }), row({ sort_order: 1 })])).toBe(4);
  });

  it("swaps adjacent rows and refuses to leave the list", () => {
    const rows = [row({ model: "a", sort_order: 0 }), row({ model: "b", sort_order: 1 })];
    expect(swapSortOrders(rows, 0, "down")).toEqual([0, 1]);
    expect(swapSortOrders(rows, 1, "up")).toEqual([1, 0]);
    expect(swapSortOrders(rows, 0, "up")).toBeNull();
    expect(swapSortOrders(rows, 1, "down")).toBeNull();
  });
});
