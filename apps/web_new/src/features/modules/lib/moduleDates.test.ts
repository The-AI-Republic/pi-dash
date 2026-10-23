// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Calendar-day readings for module dates (§4.1, §3.5, §3.6): days never
// shift across zones, malformed values read as empty, ranges collapse the
// shared month/year, and the reader-zone note appears only when the zones
// differ and both are known.

import { afterEach, describe, expect, it } from "vitest";

import {
  formatModuleDay,
  formatModuleRange,
  moduleDateText,
  moduleRangeDurationDays,
  parseModuleDay,
  userZoneDiffers,
} from "./moduleDates.js";

const originalResolved = Intl.DateTimeFormat.prototype.resolvedOptions;

afterEach(() => {
  Intl.DateTimeFormat.prototype.resolvedOptions = originalResolved;
});

describe("parseModuleDay", () => {
  it("parses a calendar day", () => {
    expect(parseModuleDay("2026-10-15")).toEqual({ year: 2026, month: 10, day: 15 });
    expect(parseModuleDay(" 2026-01-02 ")).toEqual({ year: 2026, month: 1, day: 2 });
  });

  it("rejects absent, malformed and impossible days", () => {
    expect(parseModuleDay(null)).toBeNull();
    expect(parseModuleDay(undefined)).toBeNull();
    expect(parseModuleDay("")).toBeNull();
    expect(parseModuleDay("2026-1-5")).toBeNull();
    expect(parseModuleDay("15/10/2026")).toBeNull();
    expect(parseModuleDay("2026-13-01")).toBeNull();
    expect(parseModuleDay("2026-02-31")).toBeNull();
    expect(parseModuleDay("2026-00-10")).toBeNull();
  });

  it("accepts a leap day only in a leap year", () => {
    expect(parseModuleDay("2028-02-29")).toEqual({ year: 2028, month: 2, day: 29 });
    expect(parseModuleDay("2027-02-29")).toBeNull();
  });
});

describe("formatModuleDay", () => {
  it("reads the day, month and year", () => {
    expect(formatModuleDay({ year: 2026, month: 10, day: 5 })).toBe("Oct 05, 2026");
    expect(formatModuleDay({ year: 2025, month: 12, day: 31 })).toBe("Dec 31, 2025");
  });
});

describe("formatModuleRange", () => {
  it("collapses a range inside one month", () => {
    expect(formatModuleRange({ start: "2026-10-15", target: "2026-10-22" })).toBe("Oct 15 - 22, 2026");
  });

  it("names both months inside one year", () => {
    expect(formatModuleRange({ start: "2026-10-30", target: "2026-11-04" })).toBe("Oct 30 - Nov 04, 2026");
  });

  it("repeats both years when the range crosses one", () => {
    expect(formatModuleRange({ start: "2026-12-30", target: "2027-01-04" })).toBe("Dec 30, 2026 - Jan 04, 2027");
  });

  it("reads one side as one day and neither side as nothing", () => {
    expect(formatModuleRange({ start: "2026-10-15", target: null })).toBe("Oct 15, 2026");
    expect(formatModuleRange({ start: null, target: "2026-10-22" })).toBe("Oct 22, 2026");
    expect(formatModuleRange({ start: null, target: null })).toBeNull();
    expect(formatModuleRange({ start: "bogus", target: null })).toBeNull();
  });
});

describe("moduleRangeDurationDays", () => {
  it("counts both ends", () => {
    expect(moduleRangeDurationDays({ start: "2026-10-01", target: "2026-10-03" })).toBe(3);
    expect(moduleRangeDurationDays({ start: "2026-10-03", target: "2026-10-01" })).toBe(-1);
    expect(moduleRangeDurationDays({ start: "2026-01-01", target: "2027-01-01" })).toBe(366);
  });

  it("reads null when a side is missing or malformed", () => {
    expect(moduleRangeDurationDays({ start: "2026-10-01", target: null })).toBeNull();
    expect(moduleRangeDurationDays({ start: "nope", target: "2026-10-01" })).toBeNull();
  });
});

describe("moduleDateText", () => {
  it("reads the day or the caller's placeholder", () => {
    expect(moduleDateText("2026-10-15", "Start date")).toBe("Oct 15, 2026");
    expect(moduleDateText(null, "Start date")).toBe("Start date");
    expect(moduleDateText("bad", "End date")).toBe("End date");
  });
});

describe("userZoneDiffers", () => {
  it("compares the project zone with the reader's", () => {
    expect(userZoneDiffers("Asia/Tokyo", "Asia/Tokyo")).toBe(false);
    expect(userZoneDiffers("Asia/Tokyo", "America/Los_Angeles")).toBe(true);
  });

  it("reads the browser zone when the caller names none", () => {
    Intl.DateTimeFormat.prototype.resolvedOptions = function resolved(this: Intl.DateTimeFormat) {
      return { ...originalResolved.call(this), timeZone: "Europe/Berlin" };
    } as typeof Intl.DateTimeFormat.prototype.resolvedOptions;
    expect(userZoneDiffers("Europe/Berlin")).toBe(false);
    expect(userZoneDiffers("Asia/Tokyo")).toBe(true);
  });

  it("never claims a difference it cannot establish", () => {
    expect(userZoneDiffers(null, "Asia/Tokyo")).toBe(false);
    expect(userZoneDiffers(undefined, "Asia/Tokyo")).toBe(false);
    expect(userZoneDiffers("   ", "Asia/Tokyo")).toBe(false);
    expect(userZoneDiffers("Asia/Tokyo", "")).toBe(false);
    Intl.DateTimeFormat.prototype.resolvedOptions = function resolved(this: Intl.DateTimeFormat) {
      const { timeZone: _omitted, ...rest } = originalResolved.call(this);
      return rest;
    } as typeof Intl.DateTimeFormat.prototype.resolvedOptions;
    expect(userZoneDiffers("Asia/Tokyo")).toBe(false);
  });

  it("reads no browser zone when the engine cannot name one", () => {
    Intl.DateTimeFormat.prototype.resolvedOptions = function resolved(): Intl.ResolvedDateTimeFormatOptions {
      throw new Error("no zone");
    } as typeof Intl.DateTimeFormat.prototype.resolvedOptions;
    expect(userZoneDiffers("Asia/Tokyo")).toBe(false);
  });
});
