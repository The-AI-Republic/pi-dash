// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The stored instant renders "MMM dd, yyyy" in the member timezone and
// shifts with the zone (NEWFRONT-859); absent or broken zones degrade
// to the system local zone instead of throwing.

import { describe, expect, it } from "vitest";

import { formatMemberDate } from "./memberDates.js";

// A stored midnight-UTC instant: Kiritimati (+14) reads it a day ahead,
// Midway (-11) a day behind, so every zone below renders differently.
const STORED = "2026-11-19T00:00:00.000Z";

function expected(stored: string, timeZone: string): string {
  return new Intl.DateTimeFormat("en-US", {
    month: "short",
    day: "2-digit",
    year: "numeric",
    timeZone,
  }).format(new Date(stored));
}

describe("formatMemberDate", () => {
  it("renders the stored instant in the member timezone", () => {
    expect(formatMemberDate(STORED, "Pacific/Kiritimati")).toBe(expected(STORED, "Pacific/Kiritimati"));
    expect(formatMemberDate(STORED, "Pacific/Midway")).toBe(expected(STORED, "Pacific/Midway"));
    expect(formatMemberDate(STORED, "UTC")).toBe(expected(STORED, "UTC"));
  });

  it("shifts the reading when the member timezone changes", () => {
    const kiritimati = formatMemberDate(STORED, "Pacific/Kiritimati");
    const midway = formatMemberDate(STORED, "Pacific/Midway");
    const utc = formatMemberDate(STORED, "UTC");
    expect(kiritimati).toBe("Nov 19, 2026");
    expect(midway).toBe("Nov 18, 2026");
    expect(utc).toBe("Nov 19, 2026");
    expect(midway).not.toBe(kiritimati);
  });

  it("renders a stored instant carrying its own offset", () => {
    const stored = "2026-11-19T10:30:00+05:30";
    expect(formatMemberDate(stored, "Pacific/Midway")).toBe(expected(stored, "Pacific/Midway"));
  });

  it("falls back to the system zone when the member zone is absent", () => {
    const local = new Intl.DateTimeFormat("en-US", {
      month: "short",
      day: "2-digit",
      year: "numeric",
    }).format(new Date(STORED));
    expect(formatMemberDate(STORED, null)).toBe(local);
    expect(formatMemberDate(STORED, undefined)).toBe(local);
    expect(formatMemberDate(STORED, "")).toBe(local);
    expect(formatMemberDate(STORED, "   ")).toBe(local);
  });

  it("falls back to the system zone when the member zone is broken", () => {
    const local = new Intl.DateTimeFormat("en-US", {
      month: "short",
      day: "2-digit",
      year: "numeric",
    }).format(new Date(STORED));
    expect(formatMemberDate(STORED, "Mars/Olympus")).toBe(local);
  });
});
