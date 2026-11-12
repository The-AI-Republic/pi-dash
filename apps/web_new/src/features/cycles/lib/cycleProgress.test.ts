// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Ring arithmetic: live rows read their counters, finished rows read
// the frozen snapshot, cancelled items leave the denominator, and the
// edges (zero denominator, over-completion) stay honest.

import { describe, expect, it } from "vitest";

import { cycleProgressCounts, cycleProgressPercent } from "./cycleProgress.js";

describe("cycle progress", () => {
  it("reads completed over total-minus-cancelled, rounded", () => {
    expect(cycleProgressPercent({ total_issues: 4, completed_issues: 1, cancelled_issues: 0 })).toBe(25);
    expect(cycleProgressPercent({ total_issues: 3, completed_issues: 1, cancelled_issues: 0 })).toBe(33);
    expect(cycleProgressPercent({ total_issues: 6, completed_issues: 2, cancelled_issues: 2 })).toBe(50);
  });

  it("reads 100 only when the counts exactly agree", () => {
    expect(cycleProgressPercent({ total_issues: 2, completed_issues: 2, cancelled_issues: 0 })).toBe(100);
  });

  it("renders 0% on a zero denominator instead of a full ring", () => {
    expect(cycleProgressPercent({})).toBe(0);
    expect(cycleProgressPercent({ total_issues: 0, completed_issues: 0, cancelled_issues: 0 })).toBe(0);
    expect(cycleProgressPercent({ total_issues: 2, completed_issues: 0, cancelled_issues: 2 })).toBe(0);
  });

  it("clamps over-completion to 100 and negative completed to 0", () => {
    expect(cycleProgressPercent({ total_issues: 2, completed_issues: 5, cancelled_issues: 0 })).toBe(100);
    expect(cycleProgressPercent({ total_issues: 2, completed_issues: -1, cancelled_issues: 0 })).toBe(0);
  });

  it("treats missing and non-numeric counters as zero", () => {
    expect(cycleProgressPercent({ total_issues: 4, completed_issues: 2 })).toBe(50);
    expect(
      cycleProgressPercent({ total_issues: Number.NaN, completed_issues: 2, cancelled_issues: "1" as never })
    ).toBe(0);
  });

  it("prefers a non-empty frozen snapshot over the live row", () => {
    expect(
      cycleProgressPercent({
        total_issues: 10,
        completed_issues: 9,
        cancelled_issues: 0,
        progress_snapshot: { total_issues: 4, completed_issues: 1, cancelled_issues: 0 },
      })
    ).toBe(25);
  });

  it("ignores a missing, null, or empty snapshot", () => {
    const row = { total_issues: 4, completed_issues: 2, cancelled_issues: 0 };
    expect(cycleProgressPercent({ ...row, progress_snapshot: undefined })).toBe(50);
    expect(cycleProgressPercent({ ...row, progress_snapshot: null })).toBe(50);
    expect(cycleProgressPercent({ ...row, progress_snapshot: {} })).toBe(50);
  });

  it("exposes the counts behind the ring for the peek body", () => {
    expect(cycleProgressCounts({ total_issues: 6, completed_issues: 2, cancelled_issues: 2 })).toEqual({
      completed: 2,
      total: 4,
    });
    expect(
      cycleProgressCounts({
        total_issues: 9,
        completed_issues: 8,
        cancelled_issues: 0,
        progress_snapshot: { total_issues: 5, completed_issues: 1, cancelled_issues: 1 },
      })
    ).toEqual({ completed: 1, total: 4 });
  });
});
