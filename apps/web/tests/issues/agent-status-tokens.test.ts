import { describe, expect, it } from "vitest";
import type { TIssueAgentStatus } from "@pi-dash/types";
import { formatTokenCount, resolveIssueTokenTotal } from "../../core/components/issues/issue-detail/agent-status";

function agentStatus(overrides: Partial<TIssueAgentStatus> = {}): TIssueAgentStatus {
  return {
    ticker: null,
    active_run: null,
    latest_run: null,
    run_count: 0,
    ...overrides,
  };
}

// Only the fields the token arithmetic reads; the full run summary is large and
// none of the rest participates.
function run(total_tokens: number | null, liveTotal?: number | null) {
  return {
    total_tokens,
    live_state: liveTotal === undefined ? null : { total_tokens: liveTotal },
  } as unknown as NonNullable<TIssueAgentStatus["active_run"]>;
}

describe("formatTokenCount", () => {
  it("prints small counts in full", () => {
    expect(formatTokenCount(0)).toBe("0");
    expect(formatTokenCount(1)).toBe("1");
    expect(formatTokenCount(999)).toBe("999");
  });

  it("abbreviates thousands and millions rather than printing raw digits", () => {
    expect(formatTokenCount(1000)).toBe("1.0k");
    expect(formatTokenCount(12_800)).toBe("12.8k");
    expect(formatTokenCount(1_200_000)).toBe("1.2M");
  });

  it("carries to the next unit instead of printing '1000.0k'", () => {
    expect(formatTokenCount(999_949)).toBe("999.9k");
    expect(formatTokenCount(999_950)).toBe("1.0M");
  });

  it("never renders NaN or a negative for a junk value", () => {
    expect(formatTokenCount(Number.NaN)).toBe("0");
    expect(formatTokenCount(-5)).toBe("0");
  });
});

describe("resolveIssueTokenTotal", () => {
  it("uses the server-side sum when no run is in flight", () => {
    expect(resolveIssueTokenTotal(agentStatus({ total_tokens: 12_800, run_count: 3 }))).toBe(12_800);
  });

  it("reports 0 — not null — for an issue whose runs never reported usage", () => {
    // The tile must render "0" rather than vanish or blank out.
    expect(resolveIssueTokenTotal(agentStatus({ total_tokens: 0, run_count: 2 }))).toBe(0);
  });

  it("adds the whole live total while a run is in flight, since its row is unwritten", () => {
    // A running row has no usage yet, so the server's sum covers the two
    // finished runs only; the live state carries the run in progress.
    const status = agentStatus({ total_tokens: 10_000, run_count: 3, active_run: run(null, 2_500) });
    expect(resolveIssueTokenTotal(status)).toBe(12_500);
  });

  it("adds only the delta for a resumed run whose row already carries a total", () => {
    // Pausing wrote 2,000 onto the row, so the sum already counts it; the live
    // state is now at 3,200. Adding the full live total would double count.
    const status = agentStatus({ total_tokens: 10_000, run_count: 3, active_run: run(2_000, 3_200) });
    expect(resolveIssueTokenTotal(status)).toBe(11_200);
  });

  it("adds nothing when the row has caught up with or overtaken the live state", () => {
    const status = agentStatus({ total_tokens: 10_000, run_count: 3, active_run: run(4_000, 3_200) });
    expect(resolveIssueTokenTotal(status)).toBe(10_000);
  });

  it("drops the tile for a payload with no token figure anywhere", () => {
    // An older API that predates the cumulative sum, with nothing live either.
    expect(resolveIssueTokenTotal(agentStatus({ run_count: 2 }))).toBeNull();
    expect(resolveIssueTokenTotal(null)).toBeNull();
    expect(resolveIssueTokenTotal(undefined)).toBeNull();
  });

  it("still shows the live total when the server sum is absent", () => {
    expect(resolveIssueTokenTotal(agentStatus({ run_count: 1, active_run: run(null, 900) }))).toBe(900);
  });
});
