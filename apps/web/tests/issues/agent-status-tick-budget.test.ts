/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { describe, expect, it, vi } from "vitest";
import type { TIssueAgentTicker } from "@pi-dash/types";

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

import { formatTickBudget } from "../../core/components/issues/issue-detail/agent-status";

/** Substitute `{name}` placeholders, matching the app's translation shim. */
const t = ((key: string, vars?: Record<string, unknown>) =>
  vars ? key.replace(/\{(\w+)\}/g, (_m, k) => String(vars[k] ?? "")) : key) as never;

function ticker(over: Partial<TIssueAgentTicker>): TIssueAgentTicker {
  return {
    enabled: true,
    user_disabled: false,
    used: 0,
    tick_count: 0,
    max_ticks: 10,
    interval_seconds: 10800,
    next_run_at: null,
    last_tick_at: null,
    ...over,
  };
}

describe("formatTickBudget", () => {
  it("reports the plain pool when nothing has waited", () => {
    expect(formatTickBudget(ticker({ used: 6, max_ticks: 10 }), t)).toBe("6 of 10 runs used");
  });

  it("shows waits separately and never folds them into the pool", () => {
    // Six runs of work and four waits. Each wait added one to `used` (the run
    // it ended) and one to `max_ticks` (the tick it bought back), so the raw
    // payload is 10 of 14 — which would wrongly read as a nearly-spent pool.
    const budget = formatTickBudget(ticker({ used: 10, waited: 4, max_ticks: 14 }), t);
    expect(budget).toBe("6 of 10 runs used, 4 waits");
    expect(budget).not.toContain("14");
  });

  it("keeps the pool steady as waits accumulate", () => {
    // The whole point of the wait budget: waiting does not eat the pool.
    expect(formatTickBudget(ticker({ used: 3, waited: 1, max_ticks: 11 }), t)).toBe("2 of 10 runs used, 1 wait");
    expect(formatTickBudget(ticker({ used: 12, waited: 10, max_ticks: 20 }), t)).toBe("2 of 10 runs used, 10 waits");
  });

  it("falls back to the pre-pool tick_count spelling", () => {
    expect(formatTickBudget(ticker({ used: undefined as never, tick_count: 4 }), t)).toBe("4 of 10 runs used");
  });

  it("reports an uncapped pool without wait arithmetic", () => {
    expect(formatTickBudget(ticker({ used: 7, waited: 2, max_ticks: -1 }), t)).toBe("7 runs used, no cap");
  });

  it("returns null without a ticker", () => {
    expect(formatTickBudget(null, t)).toBeNull();
    expect(formatTickBudget(undefined, t)).toBeNull();
  });
});
