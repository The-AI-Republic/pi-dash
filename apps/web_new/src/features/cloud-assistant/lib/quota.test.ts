// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Free-lane meter wording (NEWPIDASH-149): the abbreviations the
// sentence shows, and the reset rendered in local time.
import { describe, expect, it } from "vitest";

import { formatResetTime, formatTokenCount, freeTokenMeterText } from "./quota.js";

function localHHMM(iso: string): string {
  const instant = new Date(iso);
  const pad = (value: number): string => String(value).padStart(2, "0");
  return `${pad(instant.getHours())}:${pad(instant.getMinutes())}`;
}

describe("formatTokenCount", () => {
  it("abbreviates the allowances the sentence carries", () => {
    expect(formatTokenCount(2_000_000)).toBe("2M");
    expect(formatTokenCount(5_000_000)).toBe("5M");
    expect(formatTokenCount(1_500_000)).toBe("1.5M");
    expect(formatTokenCount(123_456)).toBe("123.5K");
    expect(formatTokenCount(1_250)).toBe("1.3K");
    expect(formatTokenCount(999)).toBe("999");
    expect(formatTokenCount(0)).toBe("0");
  });

  it("never renders a broken count", () => {
    expect(formatTokenCount(Number.NaN)).toBe("0");
    expect(formatTokenCount(-5)).toBe("0");
    expect(formatTokenCount(Number.POSITIVE_INFINITY)).toBe("0");
  });
});

describe("formatResetTime", () => {
  it("renders the reset in local time", () => {
    expect(formatResetTime("2026-11-10T00:00:00+00:00")).toBe(localHHMM("2026-11-10T00:00:00+00:00"));
  });

  it("declines an unreadable instant", () => {
    expect(formatResetTime("not-a-date")).toBeNull();
    expect(formatResetTime("")).toBeNull();
  });
});

describe("freeTokenMeterText", () => {
  it("states the counts and the local reset", () => {
    expect(freeTokenMeterText(123_456, 2_000_000, "2026-11-10T00:00:00+00:00")).toBe(
      `123.5K of 2M free tokens used today, resets at ${localHHMM("2026-11-10T00:00:00+00:00")} (local)`
    );
  });

  it("keeps the counts when the reset cannot be read", () => {
    expect(freeTokenMeterText(5, 2_000_000, "garbage")).toBe("5 of 2M free tokens used today");
  });
});
