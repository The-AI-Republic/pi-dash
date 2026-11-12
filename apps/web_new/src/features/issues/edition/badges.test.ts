// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud detail accents (ISS-236): the overdue reading and the transfer-hop
// sentence, plus the flag that decides whether either may render.
import { describe, expect, it } from "vitest";

import { DETAIL_BADGES_FLAG, isEditionFlagOn } from "./flags.js";
import { overdueBadgeLabel, overdueDays, showsDetailAccents, transferHopNotice } from "./badges.js";

describe("showsDetailAccents", () => {
  it("stays off in the community build", () => {
    expect(showsDetailAccents()).toBe(false);
    expect(isEditionFlagOn(DETAIL_BADGES_FLAG)).toBe(false);
  });

  it("turns on with the detail-badges flag", () => {
    expect(showsDetailAccents({ [DETAIL_BADGES_FLAG]: true })).toBe(true);
  });
});

describe("overdueDays", () => {
  it("counts whole days late", () => {
    expect(overdueDays("2026-01-01", "2026-01-04")).toBe(-3);
  });

  it("is positive for a future day and zero for today", () => {
    expect(overdueDays("2026-01-05", "2026-01-04")).toBe(1);
    expect(overdueDays("2026-01-04", "2026-01-04")).toBe(0);
  });

  it("compares calendar days, ignoring any time in the input", () => {
    expect(overdueDays("2026-01-01T23:30:00Z", "2026-01-02T00:10:00Z")).toBe(-1);
  });

  it("answers zero for an unreadable day rather than guessing", () => {
    expect(overdueDays("not-a-day", "2026-01-04")).toBe(0);
    expect(overdueDays("2026-01-04", "")).toBe(0);
  });
});

describe("overdueBadgeLabel", () => {
  it("names today", () => {
    expect(overdueBadgeLabel("2026-01-04", "2026-01-04")).toBe("Due today");
  });

  it("singularizes one day", () => {
    expect(overdueBadgeLabel("2026-01-03", "2026-01-04")).toBe("1 day overdue");
  });

  it("pluralizes more than one day", () => {
    expect(overdueBadgeLabel("2026-01-01", "2026-01-04")).toBe("3 days overdue");
  });

  it("reads empty for a future due day", () => {
    expect(overdueBadgeLabel("2026-02-01", "2026-01-04")).toBe("");
  });
});

describe("transferHopNotice", () => {
  it("names the destination cycle", () => {
    expect(transferHopNotice("Sprint 4")).toBe("Moved to Sprint 4.");
  });
});
